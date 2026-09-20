#include <cuda_runtime.h>
#include <cuda_bf16.h>
#include <math.h>
#include <stdint.h>
#include <stdio.h>

// Parameterized autotuning search space with compile-time overrides
#ifndef PAGE_SIZE
#define PAGE_SIZE 16
#endif

#ifndef BLOCK_THREADS
#define BLOCK_THREADS 128
#endif

#ifndef Q_HEADS_PER_BLOCK
#define Q_HEADS_PER_BLOCK 4
#endif

#ifndef VECTOR_WIDTH
#define VECTOR_WIDTH 2
#endif

#ifndef PREFETCH_DISTANCE
#define PREFETCH_DISTANCE 1
#endif

#define WARP_SIZE 32
#if defined(Q_HEADS_PER_BLOCK) && (Q_HEADS_PER_BLOCK > 0)
#define WARPS_PER_BLOCK Q_HEADS_PER_BLOCK
#else
#define WARPS_PER_BLOCK (BLOCK_THREADS / WARP_SIZE)
#endif

__device__ __forceinline__ float bf16_to_f32(__nv_bfloat16 v) {
    return __bfloat162float(v);
}

__device__ __forceinline__ __nv_bfloat16 f32_to_bf16(float v) {
    return __float2bfloat16(v);
}

__device__ __forceinline__ void prefetch_global_l2(const void *ptr) {
#if defined(__CUDA_ARCH__) && (__CUDA_ARCH__ >= 700)
    asm volatile("prefetch.global.L2 [%0];" :: "l"(ptr));
#endif
}

// Truly Cooperative Grouped Query Paged Attention Kernel for Blackwell sm_121.
// Each warp processes one query head across 32 lanes cooperatively.
// Coalesced 128-byte vectorized (32-bit __nv_bfloat162) memory accesses.
// Online softmax is maintained in registers; dot products reduce via __shfl_down_sync.
__global__ void paged_attention_bf16_cooperative_kernel(
    const __nv_bfloat16 * __restrict__ q,
    const __nv_bfloat16 * __restrict__ k_pool,
    const __nv_bfloat16 * __restrict__ v_pool,
    const int32_t * __restrict__ block_tables,
    const int32_t * __restrict__ context_lens,
    int32_t max_blocks_per_seq,
    int32_t num_seqs,
    int32_t num_q_heads,
    int32_t num_kv_heads,
    int32_t head_dim,
    float sm_scale,
    __nv_bfloat16 * __restrict__ out
) {
    int warp_id = threadIdx.x / WARP_SIZE;
    int lane_id = threadIdx.x % WARP_SIZE;

    int q_head_idx = blockIdx.x * WARPS_PER_BLOCK + warp_id;
    int seq_idx = blockIdx.y;

    if (seq_idx >= num_seqs || q_head_idx >= num_q_heads) {
        return;
    }

    int context_len = context_lens[seq_idx];
    if (context_len <= 0) {
        return;
    }

    int heads_per_group = num_q_heads / num_kv_heads;
    int kv_head_idx = q_head_idx / heads_per_group;

    const __nv_bfloat16 *q_vec = q + ((seq_idx * num_q_heads + q_head_idx) * head_dim);

    // Shared memory for query vectors (one slice per warp)
    extern __shared__ float s_q_all[];
    float *s_q = s_q_all + (warp_id * head_dim);

    // Load Q into shared memory (coalesced pairs)
    for (int p = 0; p < (head_dim + 63) / 64; ++p) {
        int d = lane_id * 2 + p * 64;
        if (d + 1 < head_dim) {
            const __nv_bfloat162 *q2_ptr = reinterpret_cast<const __nv_bfloat162*>(q_vec + d);
            __nv_bfloat162 q2 = *q2_ptr;
            s_q[d] = bf16_to_f32(__low2bfloat16(q2));
            s_q[d + 1] = bf16_to_f32(__high2bfloat16(q2));
        } else if (d < head_dim) {
            s_q[d] = bf16_to_f32(q_vec[d]);
        }
    }
    __syncwarp();

    float m_prev = -1e20f;
    float l_prev = 0.0f;

    // Up to 4 pairs (8 elements per lane, supporting head_dim up to 256)
    const int max_pairs = (head_dim + 63) / 64;
    float acc_out[8];
    #pragma unroll
    for (int i = 0; i < 8; ++i) {
        acc_out[i] = 0.0f;
    }

    int total_blocks = (context_len + PAGE_SIZE - 1) / PAGE_SIZE;
    const int32_t *seq_blocks = block_tables + (seq_idx * max_blocks_per_seq);

    for (int b = 0; b < total_blocks; ++b) {
        int32_t physical_block_id = seq_blocks[b];
        if (physical_block_id < 0) {
            continue;
        }

        int tokens_in_this_block = PAGE_SIZE;
        if (b == total_blocks - 1) {
            int rem = context_len % PAGE_SIZE;
            if (rem != 0) {
                tokens_in_this_block = rem;
            }
        }

        size_t block_offset = (size_t)physical_block_id * num_kv_heads * PAGE_SIZE * head_dim;
        const __nv_bfloat16 *k_block = k_pool + block_offset + (kv_head_idx * PAGE_SIZE * head_dim);
        const __nv_bfloat16 *v_block = v_pool + block_offset + (kv_head_idx * PAGE_SIZE * head_dim);

#if PREFETCH_DISTANCE > 0
        if (b + PREFETCH_DISTANCE < total_blocks) {
            int32_t next_blk_id = seq_blocks[b + PREFETCH_DISTANCE];
            if (next_blk_id >= 0) {
                size_t next_offset = (size_t)next_blk_id * num_kv_heads * PAGE_SIZE * head_dim;
                const void *next_k = k_pool + next_offset + (kv_head_idx * PAGE_SIZE * head_dim);
                const void *next_v = v_pool + next_offset + (kv_head_idx * PAGE_SIZE * head_dim);
                prefetch_global_l2(next_k);
                prefetch_global_l2(next_v);
            }
        }
#endif

        for (int tok = 0; tok < tokens_in_this_block; ++tok) {
            const __nv_bfloat16 *k_tok = k_block + (tok * head_dim);
            const __nv_bfloat16 *v_tok = v_block + (tok * head_dim);

            // Step 1: Thread-local partial dot product across lane's pairs (aligned 32-bit loads)
            float my_dot = 0.0f;
            for (int p = 0; p < max_pairs; ++p) {
                int d = lane_id * 2 + p * 64;
                if (d + 1 < head_dim) {
                    const __nv_bfloat162 *k2_ptr = reinterpret_cast<const __nv_bfloat162*>(k_tok + d);
                    __nv_bfloat162 k2 = *k2_ptr;
                    my_dot += s_q[d] * bf16_to_f32(__low2bfloat16(k2)) + s_q[d + 1] * bf16_to_f32(__high2bfloat16(k2));
                } else if (d < head_dim) {
                    my_dot += s_q[d] * bf16_to_f32(k_tok[d]);
                }
            }

            // Step 2: Cooperative warp reduction using __shfl_down_sync
            #pragma unroll
            for (int offset = 16; offset > 0; offset /= 2) {
                my_dot += __shfl_down_sync(0xffffffff, my_dot, offset);
            }

            // Step 3: Broadcast complete score to all 32 lanes
            float score = __shfl_sync(0xffffffff, my_dot, 0) * sm_scale;

            // Step 4: Online softmax update
            float m_new = fmaxf(m_prev, score);
            float alpha = expf(m_prev - m_new);
            float beta = expf(score - m_new);
            float l_new = l_prev * alpha + beta;

            // Step 5: Update value accumulator in registers for lane's dimensions (aligned loads)
            for (int p = 0; p < max_pairs; ++p) {
                int d = lane_id * 2 + p * 64;
                if (d + 1 < head_dim) {
                    const __nv_bfloat162 *v2_ptr = reinterpret_cast<const __nv_bfloat162*>(v_tok + d);
                    __nv_bfloat162 v2 = *v2_ptr;
                    acc_out[p * 2] = acc_out[p * 2] * alpha + beta * bf16_to_f32(__low2bfloat16(v2));
                    acc_out[p * 2 + 1] = acc_out[p * 2 + 1] * alpha + beta * bf16_to_f32(__high2bfloat16(v2));
                } else if (d < head_dim) {
                    acc_out[p * 2] = acc_out[p * 2] * alpha + beta * bf16_to_f32(v_tok[d]);
                }
            }

            m_prev = m_new;
            l_prev = l_new;
        }
    }

    // Step 6: Normalize by l_prev and write output cooperatively (aligned 32-bit stores)
    float inv_l = (l_prev > 0.0f) ? (1.0f / l_prev) : 0.0f;
    __nv_bfloat16 *out_vec = out + ((seq_idx * num_q_heads + q_head_idx) * head_dim);
    for (int p = 0; p < max_pairs; ++p) {
        int d = lane_id * 2 + p * 64;
        if (d + 1 < head_dim) {
            __nv_bfloat162 out2 = __halves2bfloat162(
                f32_to_bf16(acc_out[p * 2] * inv_l),
                f32_to_bf16(acc_out[p * 2 + 1] * inv_l)
            );
            *reinterpret_cast<__nv_bfloat162*>(out_vec + d) = out2;
        } else if (d < head_dim) {
            out_vec[d] = f32_to_bf16(acc_out[p * 2] * inv_l);
        }
    }
}

static bool is_device_or_managed_ptr(const void *ptr) {
    cudaPointerAttributes attr;
    cudaError_t err = cudaPointerGetAttributes(&attr, ptr);
    if (err != cudaSuccess) {
        cudaGetLastError();
        return false;
    }
    return (attr.type == cudaMemoryTypeDevice || attr.type == cudaMemoryTypeManaged);
}

// Persistent device scratch buffers to eliminate per-step cudaMalloc/cudaFree
static __nv_bfloat16 *g_scratch_d_q = NULL;
static size_t g_scratch_cap_q = 0;
static __nv_bfloat16 *g_scratch_d_out = NULL;
static size_t g_scratch_cap_out = 0;
static int32_t *g_scratch_d_bt = NULL;
static size_t g_scratch_cap_bt = 0;
static int32_t *g_scratch_d_cl = NULL;
static size_t g_scratch_cap_cl = 0;

extern "C" {

int paged_attention_bf16_forward(
    const __nv_bfloat16 *q,
    const __nv_bfloat16 *k_pool,
    const __nv_bfloat16 *v_pool,
    const int32_t *block_tables,
    const int32_t *context_lens,
    int32_t max_blocks_per_seq,
    int32_t num_seqs,
    int32_t num_q_heads,
    int32_t num_kv_heads,
    int32_t head_dim,
    float sm_scale,
    __nv_bfloat16 *out
) {
    if (num_seqs <= 0 || num_q_heads <= 0 || head_dim <= 0) {
        return 0;
    }

    bool q_dev = is_device_or_managed_ptr(q);
    bool k_dev = is_device_or_managed_ptr(k_pool);
    bool v_dev = is_device_or_managed_ptr(v_pool);
    bool bt_dev = is_device_or_managed_ptr(block_tables);
    bool cl_dev = is_device_or_managed_ptr(context_lens);
    bool out_dev = is_device_or_managed_ptr(out);

    size_t q_bytes = (size_t)num_seqs * num_q_heads * head_dim * sizeof(__nv_bfloat16);
    size_t out_bytes = q_bytes;
    size_t bt_bytes = (size_t)num_seqs * max_blocks_per_seq * sizeof(int32_t);
    size_t cl_bytes = (size_t)num_seqs * sizeof(int32_t);

    const __nv_bfloat16 *d_q = q;
    if (!q_dev) {
        if (q_bytes > g_scratch_cap_q) {
            if (g_scratch_d_q) cudaFree(g_scratch_d_q);
            cudaMalloc((void**)&g_scratch_d_q, q_bytes);
            g_scratch_cap_q = q_bytes;
        }
        cudaMemcpy((void*)g_scratch_d_q, q, q_bytes, cudaMemcpyHostToDevice);
        d_q = g_scratch_d_q;
    }

    const int32_t *d_bt = block_tables;
    if (!bt_dev) {
        if (bt_bytes > g_scratch_cap_bt) {
            if (g_scratch_d_bt) cudaFree(g_scratch_d_bt);
            cudaMalloc((void**)&g_scratch_d_bt, bt_bytes);
            g_scratch_cap_bt = bt_bytes;
        }
        cudaMemcpy((void*)g_scratch_d_bt, block_tables, bt_bytes, cudaMemcpyHostToDevice);
        d_bt = g_scratch_d_bt;
    }

    const int32_t *d_cl = context_lens;
    if (!cl_dev) {
        if (cl_bytes > g_scratch_cap_cl) {
            if (g_scratch_d_cl) cudaFree(g_scratch_d_cl);
            cudaMalloc((void**)&g_scratch_d_cl, cl_bytes);
            g_scratch_cap_cl = cl_bytes;
        }
        cudaMemcpy((void*)g_scratch_d_cl, context_lens, cl_bytes, cudaMemcpyHostToDevice);
        d_cl = g_scratch_d_cl;
    }

    __nv_bfloat16 *d_out = out;
    if (!out_dev) {
        if (out_bytes > g_scratch_cap_out) {
            if (g_scratch_d_out) cudaFree(g_scratch_d_out);
            cudaMalloc((void**)&g_scratch_d_out, out_bytes);
            g_scratch_cap_out = out_bytes;
        }
        d_out = g_scratch_d_out;
    }

    const __nv_bfloat16 *d_k = k_pool;
    const __nv_bfloat16 *d_v = v_pool;
    __nv_bfloat16 *temp_k = NULL;
    __nv_bfloat16 *temp_v = NULL;

    if (!k_dev || !v_dev) {
        int max_block_id = 0;
        for (int i = 0; i < num_seqs * max_blocks_per_seq; ++i) {
            if (block_tables[i] > max_block_id) {
                max_block_id = block_tables[i];
            }
        }
        size_t total_pages = max_block_id + 1;
        size_t pool_bytes = total_pages * num_kv_heads * PAGE_SIZE * head_dim * sizeof(__nv_bfloat16);

        if (!k_dev) {
            cudaMalloc((void**)&temp_k, pool_bytes);
            cudaMemcpy((void*)temp_k, k_pool, pool_bytes, cudaMemcpyHostToDevice);
            d_k = temp_k;
        }
        if (!v_dev) {
            cudaMalloc((void**)&temp_v, pool_bytes);
            cudaMemcpy((void*)temp_v, v_pool, pool_bytes, cudaMemcpyHostToDevice);
            d_v = temp_v;
        }
    }

    int warps_per_block = WARPS_PER_BLOCK;
    int block_threads = warps_per_block * WARP_SIZE;
    int blocks_x = (num_q_heads + warps_per_block - 1) / warps_per_block;
    dim3 grid(blocks_x, num_seqs);
    dim3 block(block_threads);
    size_t shared_mem = warps_per_block * head_dim * sizeof(float);

    paged_attention_bf16_cooperative_kernel<<<grid, block, shared_mem>>>(
        d_q,
        d_k,
        d_v,
        d_bt,
        d_cl,
        max_blocks_per_seq,
        num_seqs,
        num_q_heads,
        num_kv_heads,
        head_dim,
        sm_scale,
        d_out
    );

    cudaError_t err = cudaDeviceSynchronize();
    if (err != cudaSuccess) {
        fprintf(stderr, "paged_attention_bf16_cooperative_kernel sync failed: %s\n", cudaGetErrorString(err));
        if (temp_k) cudaFree(temp_k);
        if (temp_v) cudaFree(temp_v);
        return -1;
    }

    if (!out_dev) {
        cudaMemcpy(out, d_out, out_bytes, cudaMemcpyDeviceToHost);
    }

    if (temp_k) cudaFree(temp_k);
    if (temp_v) cudaFree(temp_v);

    return 0;
}

void paged_attention_bf16_destroy(void) {
    if (g_scratch_d_q) { cudaFree(g_scratch_d_q); g_scratch_d_q = NULL; g_scratch_cap_q = 0; }
    if (g_scratch_d_out) { cudaFree(g_scratch_d_out); g_scratch_d_out = NULL; g_scratch_cap_out = 0; }
    if (g_scratch_d_bt) { cudaFree(g_scratch_d_bt); g_scratch_d_bt = NULL; g_scratch_cap_bt = 0; }
    if (g_scratch_d_cl) { cudaFree(g_scratch_d_cl); g_scratch_d_cl = NULL; g_scratch_cap_cl = 0; }
}

}
