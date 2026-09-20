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
#define Q_HEADS_PER_BLOCK 7
#endif

#ifndef VECTOR_WIDTH
#define VECTOR_WIDTH 2
#endif

#ifndef PREFETCH_DISTANCE
#define PREFETCH_DISTANCE 1
#endif

__device__ __forceinline__ float bf16_to_f32(__nv_bfloat16 v) {
    return __bfloat162float(v);
}

__device__ __forceinline__ __nv_bfloat16 f32_to_bf16(float v) {
    return __float2bfloat16(v);
}

// Grouped Query Paged Attention Kernel for Blackwell sm_121
__global__ void paged_attention_bf16_kernel(
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
    int q_head_idx = blockIdx.x;
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

    extern __shared__ float s_q[];
    for (int d = threadIdx.x; d < head_dim; d += blockDim.x) {
        s_q[d] = bf16_to_f32(q_vec[d]);
    }
    __syncthreads();

    float m_prev = -1e20f;
    float l_prev = 0.0f;

    float acc_out[256];
    for (int d = 0; d < head_dim; ++d) {
        acc_out[d] = 0.0f;
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

        for (int tok = 0; tok < tokens_in_this_block; ++tok) {
            const __nv_bfloat16 *k_tok = k_block + (tok * head_dim);
            const __nv_bfloat16 *v_tok = v_block + (tok * head_dim);

            float score = 0.0f;
            for (int d = 0; d < head_dim; ++d) {
                score += s_q[d] * bf16_to_f32(k_tok[d]);
            }
            score *= sm_scale;

            float m_new = fmaxf(m_prev, score);
            float alpha = expf(m_prev - m_new);
            float beta = expf(score - m_new);
            float l_new = l_prev * alpha + beta;

            for (int d = 0; d < head_dim; ++d) {
                acc_out[d] = acc_out[d] * alpha + beta * bf16_to_f32(v_tok[d]);
            }

            m_prev = m_new;
            l_prev = l_new;
        }
    }

    if (threadIdx.x == 0) {
        float inv_l = (l_prev > 0.0f) ? (1.0f / l_prev) : 0.0f;
        __nv_bfloat16 *out_vec = out + ((seq_idx * num_q_heads + q_head_idx) * head_dim);
        for (int d = 0; d < head_dim; ++d) {
            out_vec[d] = f32_to_bf16(acc_out[d] * inv_l);
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

    const __nv_bfloat16 *d_q = q;
    const __nv_bfloat16 *d_k = k_pool;
    const __nv_bfloat16 *d_v = v_pool;
    const int32_t *d_bt = block_tables;
    const int32_t *d_cl = context_lens;
    __nv_bfloat16 *d_out = out;

    size_t q_bytes = (size_t)num_seqs * num_q_heads * head_dim * sizeof(__nv_bfloat16);
    size_t out_bytes = q_bytes;
    size_t bt_bytes = (size_t)num_seqs * max_blocks_per_seq * sizeof(int32_t);
    size_t cl_bytes = (size_t)num_seqs * sizeof(int32_t);

    int max_block_id = 0;
    for (int i = 0; i < num_seqs * max_blocks_per_seq; ++i) {
        if (block_tables[i] > max_block_id) {
            max_block_id = block_tables[i];
        }
    }
    size_t total_pages = max_block_id + 1;
    size_t pool_bytes = total_pages * num_kv_heads * PAGE_SIZE * head_dim * sizeof(__nv_bfloat16);

    if (!q_dev) {
        cudaMalloc((void**)&d_q, q_bytes);
        cudaMemcpy((void*)d_q, q, q_bytes, cudaMemcpyHostToDevice);
    }
    if (!k_dev) {
        cudaMalloc((void**)&d_k, pool_bytes);
        cudaMemcpy((void*)d_k, k_pool, pool_bytes, cudaMemcpyHostToDevice);
    }
    if (!v_dev) {
        cudaMalloc((void**)&d_v, pool_bytes);
        cudaMemcpy((void*)d_v, v_pool, pool_bytes, cudaMemcpyHostToDevice);
    }
    if (!bt_dev) {
        cudaMalloc((void**)&d_bt, bt_bytes);
        cudaMemcpy((void*)d_bt, block_tables, bt_bytes, cudaMemcpyHostToDevice);
    }
    if (!cl_dev) {
        cudaMalloc((void**)&d_cl, cl_bytes);
        cudaMemcpy((void*)d_cl, context_lens, cl_bytes, cudaMemcpyHostToDevice);
    }
    if (!out_dev) {
        cudaMalloc((void**)&d_out, out_bytes);
    }

    dim3 grid(num_q_heads, num_seqs);
    dim3 block(BLOCK_THREADS);
    size_t shared_mem = head_dim * sizeof(float);

    paged_attention_bf16_kernel<<<grid, block, shared_mem>>>(
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
        fprintf(stderr, "paged_attention_bf16_kernel sync failed: %s\n", cudaGetErrorString(err));
        return -1;
    }

    if (!out_dev) {
        cudaMemcpy(out, d_out, out_bytes, cudaMemcpyDeviceToHost);
        cudaFree(d_out);
    }
    if (!q_dev) cudaFree((void*)d_q);
    if (!k_dev) cudaFree((void*)d_k);
    if (!v_dev) cudaFree((void*)d_v);
    if (!bt_dev) cudaFree((void*)d_bt);
    if (!cl_dev) cudaFree((void*)d_cl);

    return 0;
}

}
