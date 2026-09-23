#include <cuda_runtime.h>
#include <cuda_bf16.h>
#include <cublas_v2.h>
#include <stdint.h>
#include <stdio.h>
#include <math.h>
#include <atomic>
#include "tensor_abi.h"

extern std::atomic<uint64_t> g_kernel_exec_count;

// Canonical physical KV layout descriptor matching Rust KvLayoutDesc
struct KvLayoutDesc {
    uint64_t block_stride_bytes;
    uint64_t layer_stride_bytes;
    uint64_t kv_plane_stride_bytes;
    uint64_t token_stride_bytes;
    uint64_t head_stride_bytes;

    uint64_t pool_bytes;
    uint32_t num_blocks;
    uint32_t num_layers;
    uint32_t block_size;
};

// Resident Layer Weights on GPU
struct BlackwellResidentLayerWeights {
    ResidentTensor d_norm1;      // [hidden_dim]
    ResidentTensor d_w_qkv;      // [q_dim + 2 * kv_dim, hidden_dim]
    ResidentTensor d_w_o;        // [hidden_dim, q_dim]
    ResidentTensor d_norm2;      // [hidden_dim]
    ResidentTensor d_w_gate_up;  // [2 * intermediate_dim, hidden_dim]
    ResidentTensor d_w_down;     // [hidden_dim, intermediate_dim]
};

// Resident Model Weights on GPU
struct BlackwellResidentModelWeights {
    int num_layers;
    int hidden_dim;
    int num_q_heads;
    int num_kv_heads;
    int head_dim;
    int intermediate_dim;
    int vocab_size;
    float rms_norm_eps;
    float rope_theta;

    ResidentTensor d_embed_tokens; // [vocab_size, hidden_dim]
    BlackwellResidentLayerWeights *layers;
    ResidentTensor d_final_norm;   // [hidden_dim]
    ResidentTensor d_lm_head;      // [vocab_size, hidden_dim]
};

// Persistent GPU Activation Workspace
struct BlackwellWorkspaceC {
    int max_tokens;
    float *d_x_a;         // [max_tokens, hidden_dim]
    float *d_x_b;         // [max_tokens, hidden_dim]
    float *d_x_norm;      // [max_tokens, hidden_dim]
    float *d_qkv;         // [max_tokens, q_dim + 2 * kv_dim]
    float *d_attn_out;    // [max_tokens, q_dim]
    float *d_attn_proj;   // [max_tokens, hidden_dim]
    float *d_post_norm;   // [max_tokens, hidden_dim]
    float *d_gate_up;     // [max_tokens, 2 * intermediate_dim]
    float *d_act;         // [max_tokens, intermediate_dim]
    float *d_mlp_out;     // [max_tokens, hidden_dim]

    float *d_term_x;      // [max_tokens, hidden_dim]
    float *d_logits;      // [max_tokens, vocab_size]
    uint32_t *d_tokens;   // [max_tokens]

    // Metadata GPU buffers
    int32_t *d_positions;
    int32_t *d_block_ids;
    int32_t *d_slots;
    int32_t *d_block_tables;
    int32_t *d_context_lens;
    int32_t *d_term_indices;
    int32_t *d_prefill_offsets;
};

// Batch Plan Metadata
struct BlackwellBatchPlanC {
    int num_tokens;         // M
    int decode_count;       // Nd
    int prefill_count;      // Np
    int terminal_count;     // R
    int max_blocks_per_seq;

    const uint32_t *h_input_tokens;  // [M] (optional if embeddings provided)
    const float *h_input_embeddings; // [M, hidden_dim] (optional if tokens provided)
    const int32_t *h_positions;
    const int32_t *h_block_ids;
    const int32_t *h_slots;
    const int32_t *h_decode_block_tables;
    const int32_t *h_decode_context_lens;
    const int32_t *h_term_indices;
    const int32_t *h_prefill_offsets; // [prefill_count + 1]
};

// ----------------------------------------------------------------------------
// CUDA Kernels
// ----------------------------------------------------------------------------

__global__ void embedding_lookup_kernel(
    const uint32_t * __restrict__ tokens,
    const float * __restrict__ embed_table,
    float * __restrict__ x_out,
    int M,
    int D,
    int vocab_size
) {
    int row = blockIdx.x;
    if (row >= M) return;
    uint32_t tok = tokens[row] % vocab_size;
    const float *src = embed_table + (size_t)tok * D;
    float *dst = x_out + (size_t)row * D;
    for (int i = threadIdx.x; i < D; i += blockDim.x) {
        dst[i] = src[i];
    }
}

__device__ __forceinline__ float silu_act(float x) {
    return x / (1.0f + expf(-x));
}

__global__ void rmsnorm_batch_kernel(
    const float * __restrict__ x,
    const float * __restrict__ weight,
    float * __restrict__ out,
    int M,
    int D,
    float eps
) {
    int row = blockIdx.x;
    if (row >= M) return;

    const float *row_in = x + (size_t)row * D;
    float *row_out = out + (size_t)row * D;

    float sum_sq = 0.0f;
    for (int i = threadIdx.x; i < D; i += blockDim.x) {
        float val = row_in[i];
        sum_sq += val * val;
    }

    // Warp-level reduction
    for (int offset = 16; offset > 0; offset /= 2) {
        sum_sq += __shfl_down_sync(0xffffffff, sum_sq, offset);
    }

    __shared__ float s_sum[32];
    int lane = threadIdx.x % 32;
    int wid = threadIdx.x / 32;
    if (lane == 0) {
        s_sum[wid] = sum_sq;
    }
    __syncthreads();

    if (wid == 0) {
        float b_sum = (lane < (blockDim.x / 32)) ? s_sum[lane] : 0.0f;
        for (int offset = 16; offset > 0; offset /= 2) {
            b_sum += __shfl_down_sync(0xffffffff, b_sum, offset);
        }
        if (lane == 0) {
            s_sum[0] = b_sum;
        }
    }
    __syncthreads();

    float mean_sq = s_sum[0] / (float)D;
    float rsqrt_val = rsqrtf(mean_sq + eps);

    for (int i = threadIdx.x; i < D; i += blockDim.x) {
        row_out[i] = row_in[i] * rsqrt_val * weight[i];
    }
}

__global__ void rope_and_kv_scatter_kernel(
    float * __restrict__ qkv,
    const int32_t * __restrict__ positions,
    const int32_t * __restrict__ block_ids,
    const int32_t * __restrict__ slots,
    uint8_t * __restrict__ kv_pool,
    KvLayoutDesc layout,
    int32_t layer_idx,
    int M,
    int num_q_heads,
    int num_kv_heads,
    int head_dim,
    float theta
) {
    int row = blockIdx.x;
    if (row >= M) return;

    int q_dim = num_q_heads * head_dim;
    int kv_dim = num_kv_heads * head_dim;
    int qkv_dim = q_dim + 2 * kv_dim;

    float *row_qkv = qkv + (size_t)row * qkv_dim;
    float *row_q = row_qkv;
    float *row_k = row_qkv + q_dim;
    float *row_v = row_qkv + q_dim + kv_dim;

    int pos = positions[row];

    // 1. RoPE on Q
    for (int idx = threadIdx.x; idx < (num_q_heads * (head_dim / 2)); idx += blockDim.x) {
        int h = idx / (head_dim / 2);
        int p_idx = idx % (head_dim / 2);
        int i0 = h * head_dim + 2 * p_idx;
        int i1 = i0 + 1;

        float freq = 1.0f / powf(theta, (float)(2 * p_idx) / (float)head_dim);
        float angle = (float)pos * freq;
        float cos_val = cosf(angle);
        float sin_val = sinf(angle);

        float q0 = row_q[i0];
        float q1 = row_q[i1];
        row_q[i0] = q0 * cos_val - q1 * sin_val;
        row_q[i1] = q0 * sin_val + q1 * cos_val;
    }

    // 2. RoPE on K
    for (int idx = threadIdx.x; idx < (num_kv_heads * (head_dim / 2)); idx += blockDim.x) {
        int h = idx / (head_dim / 2);
        int p_idx = idx % (head_dim / 2);
        int i0 = h * head_dim + 2 * p_idx;
        int i1 = i0 + 1;

        float freq = 1.0f / powf(theta, (float)(2 * p_idx) / (float)head_dim);
        float angle = (float)pos * freq;
        float cos_val = cosf(angle);
        float sin_val = sinf(angle);

        float k0 = row_k[i0];
        float k1 = row_k[i1];
        row_k[i0] = k0 * cos_val - k1 * sin_val;
        row_k[i1] = k0 * sin_val + k1 * cos_val;
    }
    __syncthreads();

    // 3. GPU KV Scatter to canonical physical block-major layout
    int blk = block_ids[row];
    if (blk >= 0 && kv_pool != NULL) {
        int slot = slots[row];
        uint8_t *blk_base = kv_pool + ((uint64_t)blk * layout.block_stride_bytes);
        uint8_t *layer_base = blk_base + ((uint64_t)layer_idx * layout.layer_stride_bytes);
        uint8_t *k_plane = layer_base;
        uint8_t *v_plane = layer_base + layout.kv_plane_stride_bytes;
        uint8_t *k_tok = k_plane + ((uint64_t)slot * layout.token_stride_bytes);
        uint8_t *v_tok = v_plane + ((uint64_t)slot * layout.token_stride_bytes);

        for (int idx = threadIdx.x; idx < kv_dim; idx += blockDim.x) {
            int h = idx / head_dim;
            int d = idx % head_dim;
            __nv_bfloat16 *dst_k = (__nv_bfloat16*)(k_tok + ((uint64_t)h * layout.head_stride_bytes));
            __nv_bfloat16 *dst_v = (__nv_bfloat16*)(v_tok + ((uint64_t)h * layout.head_stride_bytes));

            dst_k[d] = __float2bfloat16(row_k[idx]);
            dst_v[d] = __float2bfloat16(row_v[idx]);
        }
    }
}

// Batched decode paged attention reading directly from canonical BF16 physical pool
__global__ void batched_decode_attention_kernel(
    const float * __restrict__ qkv,
    const uint8_t * __restrict__ kv_pool,
    KvLayoutDesc layout,
    int32_t layer_idx,
    const int32_t * __restrict__ block_tables,
    const int32_t * __restrict__ context_lens,
    int max_blocks_per_seq,
    int num_seqs,
    int num_q_heads,
    int num_kv_heads,
    int head_dim,
    float sm_scale,
    float * __restrict__ attn_out
) {
    int seq_idx = blockIdx.y;
    int q_head_idx = blockIdx.x;

    if (seq_idx >= num_seqs || q_head_idx >= num_q_heads) return;

    int ctx_len = context_lens[seq_idx];
    if (ctx_len <= 0) return;

    int group_size = num_q_heads / num_kv_heads;
    int kv_head_idx = q_head_idx / group_size;

    int q_dim = num_q_heads * head_dim;
    int kv_dim = num_kv_heads * head_dim;
    int qkv_dim = q_dim + 2 * kv_dim;

    const float *q_vec = qkv + (size_t)seq_idx * qkv_dim + (size_t)q_head_idx * head_dim;
    float *out_vec = attn_out + (size_t)seq_idx * q_dim + (size_t)q_head_idx * head_dim;

    const int32_t *seq_blocks = block_tables + (size_t)seq_idx * max_blocks_per_seq;

    extern __shared__ float s_mem[];
    // Per-warp / block storage for scores
    float *s_scores = s_mem;

    float max_score = -1e30f;
    for (int t = threadIdx.x; t < ctx_len; t += blockDim.x) {
        int blk_idx = t / layout.block_size;
        int slot = t % layout.block_size;
        int phys_blk = seq_blocks[blk_idx];

        const uint8_t *blk_base = kv_pool + ((uint64_t)phys_blk * layout.block_stride_bytes);
        const uint8_t *layer_base = blk_base + ((uint64_t)layer_idx * layout.layer_stride_bytes);
        const uint8_t *k_tok = layer_base + ((uint64_t)slot * layout.token_stride_bytes);
        const __nv_bfloat16 *k_vec = (const __nv_bfloat16*)(k_tok + ((uint64_t)kv_head_idx * layout.head_stride_bytes));

        float score = 0.0f;
        for (int d = 0; d < head_dim; d++) {
            score += q_vec[d] * __bfloat162float(k_vec[d]);
        }
        score *= sm_scale;
        s_scores[t] = score;
        if (score > max_score) max_score = score;
    }

    // Block reduction for max_score
    for (int offset = 16; offset > 0; offset /= 2) {
        max_score = fmaxf(max_score, __shfl_down_sync(0xffffffff, max_score, offset));
    }
    __shared__ float s_max;
    if (threadIdx.x == 0) s_max = max_score;
    __syncthreads();
    max_score = s_max;

    // Softmax sum
    float exp_sum = 0.0f;
    for (int t = threadIdx.x; t < ctx_len; t += blockDim.x) {
        float e = expf(s_scores[t] - max_score);
        s_scores[t] = e;
        exp_sum += e;
    }
    for (int offset = 16; offset > 0; offset /= 2) {
        exp_sum += __shfl_down_sync(0xffffffff, exp_sum, offset);
    }
    __shared__ float s_sum;
    if (threadIdx.x == 0) s_sum = exp_sum;
    __syncthreads();
    exp_sum = s_sum > 0.0f ? s_sum : 1.0f;

    // Weighted sum of V
    for (int d = threadIdx.x; d < head_dim; d += blockDim.x) {
        float acc = 0.0f;
        for (int t = 0; t < ctx_len; t++) {
            float p = s_scores[t] / exp_sum;
            int blk_idx = t / layout.block_size;
            int slot = t % layout.block_size;
            int phys_blk = seq_blocks[blk_idx];

            const uint8_t *blk_base = kv_pool + ((uint64_t)phys_blk * layout.block_stride_bytes);
            const uint8_t *layer_base = blk_base + ((uint64_t)layer_idx * layout.layer_stride_bytes);
            const uint8_t *v_plane = layer_base + layout.kv_plane_stride_bytes;
            const uint8_t *v_tok = v_plane + ((uint64_t)slot * layout.token_stride_bytes);
            const __nv_bfloat16 *v_vec = (const __nv_bfloat16*)(v_tok + ((uint64_t)kv_head_idx * layout.head_stride_bytes));

            acc += p * __bfloat162float(v_vec[d]);
        }
        out_vec[d] = acc;
    }
}

// Ragged causal prefill attention
__global__ void ragged_prefill_attention_kernel(
    const float * __restrict__ qkv,
    int decode_offset,
    int num_q_heads,
    int num_kv_heads,
    int head_dim,
    float sm_scale,
    const int32_t * __restrict__ prefill_offsets,
    int prefill_count,
    float * __restrict__ attn_out
) {
    int p_idx = blockIdx.y;
    int q_head_idx = blockIdx.x;

    if (p_idx >= prefill_count || q_head_idx >= num_q_heads) return;

    int start_tok = prefill_offsets[p_idx];
    int end_tok = prefill_offsets[p_idx + 1];
    int seq_len = end_tok - start_tok;
    if (seq_len <= 0) return;

    int group_size = num_q_heads / num_kv_heads;
    int kv_head_idx = q_head_idx / group_size;

    int q_dim = num_q_heads * head_dim;
    int kv_dim = num_kv_heads * head_dim;
    int qkv_dim = q_dim + 2 * kv_dim;

    extern __shared__ float s_prefill_scores[];

    for (int t = 0; t < seq_len; t++) {
        int cur_row = decode_offset + start_tok + t;
        const float *q_vec = qkv + (size_t)cur_row * qkv_dim + (size_t)q_head_idx * head_dim;
        float *out_vec = attn_out + (size_t)cur_row * q_dim + (size_t)q_head_idx * head_dim;

        // Causal attention up to t
        float max_s = -1e30f;
        for (int past = threadIdx.x; past <= t; past += blockDim.x) {
            int past_row = decode_offset + start_tok + past;
            const float *k_vec = qkv + (size_t)past_row * qkv_dim + q_dim + (size_t)kv_head_idx * head_dim;

            float score = 0.0f;
            for (int d = 0; d < head_dim; d++) {
                score += q_vec[d] * k_vec[d];
            }
            score *= sm_scale;
            s_prefill_scores[past] = score;
            if (score > max_s) max_s = score;
        }

        // Reduction for max
        for (int offset = 16; offset > 0; offset /= 2) {
            max_s = fmaxf(max_s, __shfl_down_sync(0xffffffff, max_s, offset));
        }
        __shared__ float s_cur_max;
        if (threadIdx.x == 0) s_cur_max = max_s;
        __syncthreads();
        max_s = s_cur_max;

        float exp_sum = 0.0f;
        for (int past = threadIdx.x; past <= t; past += blockDim.x) {
            float e = expf(s_prefill_scores[past] - max_s);
            s_prefill_scores[past] = e;
            exp_sum += e;
        }
        for (int offset = 16; offset > 0; offset /= 2) {
            exp_sum += __shfl_down_sync(0xffffffff, exp_sum, offset);
        }
        __shared__ float s_cur_sum;
        if (threadIdx.x == 0) s_cur_sum = exp_sum;
        __syncthreads();
        exp_sum = s_cur_sum > 0.0f ? s_cur_sum : 1.0f;

        for (int d = threadIdx.x; d < head_dim; d += blockDim.x) {
            float acc = 0.0f;
            for (int past = 0; past <= t; past++) {
                int past_row = decode_offset + start_tok + past;
                const float *v_vec = qkv + (size_t)past_row * qkv_dim + q_dim + kv_dim + (size_t)kv_head_idx * head_dim;
                acc += (s_prefill_scores[past] / exp_sum) * v_vec[d];
            }
            out_vec[d] = acc;
        }
    }
}

__global__ void residual_add_kernel(
    const float * __restrict__ a,
    const float * __restrict__ b,
    float * __restrict__ out,
    size_t total
) {
    size_t idx = (size_t)blockIdx.x * blockDim.x + threadIdx.x;
    if (idx < total) {
        out[idx] = a[idx] + b[idx];
    }
}

__global__ void swiglu_kernel(
    const float * __restrict__ gate_up,
    float * __restrict__ act,
    int M,
    int intermediate_dim
) {
    size_t total = (size_t)M * intermediate_dim;
    size_t idx = (size_t)blockIdx.x * blockDim.x + threadIdx.x;
    if (idx >= total) return;

    int row = idx / intermediate_dim;
    int col = idx % intermediate_dim;

    size_t gate_idx = (size_t)row * (2 * intermediate_dim) + col;
    size_t up_idx = gate_idx + intermediate_dim;

    float g = gate_up[gate_idx];
    float u = gate_up[up_idx];
    act[idx] = silu_act(g) * u;
}

__global__ void gather_terminal_rows_kernel(
    const float * __restrict__ x,
    const int32_t * __restrict__ term_indices,
    float * __restrict__ term_x,
    int R,
    int D
) {
    int r = blockIdx.x;
    if (r >= R) return;
    int src_row = term_indices[r];

    const float *src = x + (size_t)src_row * D;
    float *dst = term_x + (size_t)r * D;

    for (int i = threadIdx.x; i < D; i += blockDim.x) {
        dst[i] = src[i];
    }
}

__global__ void argmax_sampling_kernel(
    const float * __restrict__ logits,
    uint32_t * __restrict__ sampled_tokens,
    int R,
    int vocab_size
) {
    int r = blockIdx.x;
    if (r >= R) return;

    const float *row = logits + (size_t)r * vocab_size;

    float max_val = -1e30f;
    int max_idx = 0;

    for (int i = threadIdx.x; i < vocab_size; i += blockDim.x) {
        float val = row[i];
        if (val > max_val) {
            max_val = val;
            max_idx = i;
        }
    }

    for (int offset = 16; offset > 0; offset /= 2) {
        float other_val = __shfl_down_sync(0xffffffff, max_val, offset);
        int other_idx = __shfl_down_sync(0xffffffff, max_idx, offset);
        if (other_val > max_val) {
            max_val = other_val;
            max_idx = other_idx;
        }
    }

    __shared__ float s_max_val[32];
    __shared__ int s_max_idx[32];

    int lane = threadIdx.x % 32;
    int wid = threadIdx.x / 32;

    if (lane == 0) {
        s_max_val[wid] = max_val;
        s_max_idx[wid] = max_idx;
    }
    __syncthreads();

    if (wid == 0) {
        float b_max_val = (lane < (blockDim.x / 32)) ? s_max_val[lane] : -1e30f;
        int b_max_idx = (lane < (blockDim.x / 32)) ? s_max_idx[lane] : 0;

        for (int offset = 16; offset > 0; offset /= 2) {
            float other_val = __shfl_down_sync(0xffffffff, b_max_val, offset);
            int other_idx = __shfl_down_sync(0xffffffff, b_max_idx, offset);
            if (other_val > b_max_val) {
                b_max_val = other_val;
                b_max_idx = other_idx;
            }
        }

        if (lane == 0) {
            sampled_tokens[r] = (uint32_t)b_max_idx;
        }
    }
}

// ----------------------------------------------------------------------------
// C-ABI Execution Functions
// ----------------------------------------------------------------------------

static bool valid_tensor_view(const TensorView *view) {
    if (!view || !view->address || view->descriptor.residency != TENSOR_HOST ||
        view->descriptor.rank == 0 || view->descriptor.rank > 4 ||
        view->descriptor.byte_len == 0 ||
        view->descriptor.byte_len > SIZE_MAX) return false;
    const TensorDescriptor &desc = view->descriptor;
    uint64_t bytes_per_element;
    switch (desc.dtype) {
        case TENSOR_F32: bytes_per_element = 4; break;
        case TENSOR_BF16: case TENSOR_FP16: bytes_per_element = 2; break;
        case TENSOR_FP8_E4M3FN: case TENSOR_FP8_E5M2:
        case TENSOR_INT8: case TENSOR_INT4: bytes_per_element = 1; break;
        default: return false;
    }
    if (desc.layout != TENSOR_ROW_MAJOR && desc.layout != TENSOR_COLUMN_MAJOR &&
        desc.layout != TENSOR_STRIDED) return false;
    if (desc.dtype == TENSOR_INT4 && desc.quant.packing == PACKING_NONE) return false;
    if (desc.quant.scheme != QUANT_NONE && desc.quant.scheme != QUANT_SYMMETRIC &&
        desc.quant.scheme != QUANT_AFFINE) return false;
    if (desc.quant.scheme == QUANT_NONE &&
        (desc.quant.scale_count || desc.quant.zero_point_count)) return false;
    if (desc.quant.scheme != QUANT_NONE &&
        (!desc.quant.scales || desc.quant.scale_count == 0)) return false;
    if (desc.quant.scale_count && !desc.quant.scales) return false;
    if (desc.quant.zero_point_count && !desc.quant.zero_points) return false;
    if (desc.quant.scale_count > SIZE_MAX / sizeof(float) ||
        desc.quant.zero_point_count > SIZE_MAX / sizeof(int32_t)) return false;
    if (desc.layout == TENSOR_ROW_MAJOR) {
        uint64_t elements = 1;
        uint64_t stride = 1;
        for (int index = (int)desc.rank - 1; index >= 0; --index) {
            if (!desc.shape[index] || desc.strides[index] != stride ||
                elements > UINT64_MAX / desc.shape[index] ||
                stride > UINT64_MAX / desc.shape[index]) return false;
            elements *= desc.shape[index];
            stride *= desc.shape[index];
        }
        if (elements > UINT64_MAX / bytes_per_element) return false;
        uint64_t expected = desc.dtype == TENSOR_INT4 ? elements / 2 + elements % 2 : elements * bytes_per_element;
        if (desc.byte_len != expected) return false;
    }
    return true;
}

static bool dense_f32_view(const TensorView *view, uint32_t rank,
                           uint64_t dim0, uint64_t dim1 = 0) {
    return valid_tensor_view(view) && view->descriptor.dtype == TENSOR_F32 &&
           view->descriptor.layout == TENSOR_ROW_MAJOR &&
           view->descriptor.quant.scheme == QUANT_NONE &&
           view->descriptor.rank == rank &&
           view->descriptor.shape[0] == dim0 &&
           (rank == 1 || view->descriptor.shape[1] == dim1);
}

static const float *dense_f32_data(const ResidentTensor &tensor) {
    return static_cast<const float *>(tensor.address);
}

static bool resident_dense_f32(const ResidentTensor &tensor) {
    return tensor.address && tensor.descriptor.residency == TENSOR_DEVICE &&
           tensor.descriptor.dtype == TENSOR_F32 &&
           tensor.descriptor.layout == TENSOR_ROW_MAJOR &&
           tensor.descriptor.quant.scheme == QUANT_NONE;
}

extern "C" {

int blackwell_tensor_upload(const TensorView *host, ResidentTensor *device,
                            cudaStream_t stream) {
    if (!device || !valid_tensor_view(host)) return -1;
    *device = {};
    device->descriptor = host->descriptor;
    device->descriptor.residency = TENSOR_DEVICE;
    device->descriptor.quant.scales = nullptr;
    device->descriptor.quant.zero_points = nullptr;
    if (cudaMalloc(&device->address, (size_t)host->descriptor.byte_len) != cudaSuccess) return -2;
    if (cudaMemcpyAsync(device->address, host->address, (size_t)host->descriptor.byte_len,
                        cudaMemcpyHostToDevice, stream) != cudaSuccess) goto failed;
    if (host->descriptor.quant.scale_count) {
        void *scales = nullptr;
        size_t bytes = (size_t)host->descriptor.quant.scale_count * sizeof(float);
        if (cudaMalloc(&scales, bytes) != cudaSuccess) goto failed;
        device->descriptor.quant.scales = static_cast<const float *>(scales);
        if (cudaMemcpyAsync(scales, host->descriptor.quant.scales, bytes,
                            cudaMemcpyHostToDevice, stream) != cudaSuccess) goto failed;
    }
    if (host->descriptor.quant.zero_point_count) {
        void *zero_points = nullptr;
        size_t bytes = (size_t)host->descriptor.quant.zero_point_count * sizeof(int32_t);
        if (cudaMalloc(&zero_points, bytes) != cudaSuccess) goto failed;
        device->descriptor.quant.zero_points = static_cast<const int32_t *>(zero_points);
        if (cudaMemcpyAsync(zero_points, host->descriptor.quant.zero_points, bytes,
                            cudaMemcpyHostToDevice, stream) != cudaSuccess) goto failed;
    }
    // Host views may reference temporary fused buffers. Finish transfer before return.
    if (cudaStreamSynchronize(stream) != cudaSuccess) goto failed;
    return 0;
failed:
    blackwell_tensor_free(device);
    return -3;
}

void blackwell_tensor_free(ResidentTensor *device) {
    if (!device) return;
    if (device->address) cudaFree(device->address);
    if (device->descriptor.quant.scales) cudaFree((void *)device->descriptor.quant.scales);
    if (device->descriptor.quant.zero_points) cudaFree((void *)device->descriptor.quant.zero_points);
    *device = {};
}

int blackwell_execute_layer(
    int layer_idx,
    const BlackwellResidentLayerWeights *weights,
    const BlackwellBatchPlanC *plan,
    BlackwellWorkspaceC *workspace,
    uint8_t *kv_pool,
    const KvLayoutDesc *layout,
    cudaStream_t stream,
    cublasHandle_t cublas_handle,
    int is_ping,
    int hidden_dim,
    int num_q_heads,
    int num_kv_heads,
    int head_dim,
    int intermediate_dim,
    float eps,
    float theta
) {
    int M = plan->num_tokens;
    if (M <= 0) return 0;
    if (!resident_dense_f32(weights->d_norm1) ||
        !resident_dense_f32(weights->d_w_qkv) ||
        !resident_dense_f32(weights->d_w_o) ||
        !resident_dense_f32(weights->d_norm2) ||
        !resident_dense_f32(weights->d_w_gate_up) ||
        !resident_dense_f32(weights->d_w_down)) return -2;

    float *d_x_in = is_ping ? workspace->d_x_b : workspace->d_x_a;
    float *d_x_out = is_ping ? workspace->d_x_a : workspace->d_x_b;

    int q_dim = num_q_heads * head_dim;
    int kv_dim = num_kv_heads * head_dim;
    int qkv_dim = q_dim + 2 * kv_dim;
    float sm_scale = 1.0f / sqrtf((float)head_dim);

    // 1. RMSNorm on x_in -> x_norm
    rmsnorm_batch_kernel<<<M, 256, 0, stream>>>(
        d_x_in, dense_f32_data(weights->d_norm1), workspace->d_x_norm, M, hidden_dim, eps
    );

    // 2. Fused QKV GEMM -> qkv
    float alpha = 1.0f, beta = 0.0f;
    cublasSgemm(cublas_handle, CUBLAS_OP_T, CUBLAS_OP_N,
                qkv_dim, M, hidden_dim,
                &alpha, dense_f32_data(weights->d_w_qkv), hidden_dim,
                workspace->d_x_norm, hidden_dim,
                &beta, workspace->d_qkv, qkv_dim);

    // 3. RoPE & GPU KV Scatter
    rope_and_kv_scatter_kernel<<<M, 128, 0, stream>>>(
        workspace->d_qkv, workspace->d_positions, workspace->d_block_ids,
        workspace->d_slots, kv_pool, *layout, layer_idx,
        M, num_q_heads, num_kv_heads, head_dim, theta
    );

    // 4. Attention
    if (plan->decode_count > 0) {
        dim3 grid_dec(num_q_heads, plan->decode_count);
        size_t s_bytes = 2048 * sizeof(float); // up to 2048 tokens context in shared mem
        batched_decode_attention_kernel<<<grid_dec, 64, s_bytes, stream>>>(
            workspace->d_qkv, kv_pool, *layout, layer_idx,
            workspace->d_block_tables, workspace->d_context_lens,
            plan->max_blocks_per_seq, plan->decode_count,
            num_q_heads, num_kv_heads, head_dim, sm_scale,
            workspace->d_attn_out
        );
    }
    if (plan->prefill_count > 0) {
        dim3 grid_pref(num_q_heads, plan->prefill_count);
        size_t s_bytes = 2048 * sizeof(float);
        ragged_prefill_attention_kernel<<<grid_pref, 64, s_bytes, stream>>>(
            workspace->d_qkv, plan->decode_count,
            num_q_heads, num_kv_heads, head_dim, sm_scale,
            workspace->d_prefill_offsets, plan->prefill_count,
            workspace->d_attn_out
        );
    }

    // 5. O Projection GEMM -> attn_proj
    cublasSgemm(cublas_handle, CUBLAS_OP_T, CUBLAS_OP_N,
                hidden_dim, M, q_dim,
                &alpha, dense_f32_data(weights->d_w_o), q_dim,
                workspace->d_attn_out, q_dim,
                &beta, workspace->d_attn_proj, hidden_dim);

    // 6. Residual Add -> x_norm (used as x_mid)
    size_t total_hidden = (size_t)M * hidden_dim;
    residual_add_kernel<<<(total_hidden + 255)/256, 256, 0, stream>>>(
        d_x_in, workspace->d_attn_proj, workspace->d_x_norm, total_hidden
    );

    // 7. Post-Attention RMSNorm -> post_norm
    rmsnorm_batch_kernel<<<M, 256, 0, stream>>>(
        workspace->d_x_norm, dense_f32_data(weights->d_norm2), workspace->d_post_norm, M, hidden_dim, eps
    );

    // 8. Fused Gate/Up GEMM -> gate_up
    cublasSgemm(cublas_handle, CUBLAS_OP_T, CUBLAS_OP_N,
                2 * intermediate_dim, M, hidden_dim,
                &alpha, dense_f32_data(weights->d_w_gate_up), hidden_dim,
                workspace->d_post_norm, hidden_dim,
                &beta, workspace->d_gate_up, 2 * intermediate_dim);

    // 9. SwiGLU -> act
    size_t total_act = (size_t)M * intermediate_dim;
    swiglu_kernel<<<(total_act + 255)/256, 256, 0, stream>>>(
        workspace->d_gate_up, workspace->d_act, M, intermediate_dim
    );

    // 10. Down Projection GEMM -> mlp_out
    cublasSgemm(cublas_handle, CUBLAS_OP_T, CUBLAS_OP_N,
                hidden_dim, M, intermediate_dim,
                &alpha, dense_f32_data(weights->d_w_down), intermediate_dim,
                workspace->d_act, intermediate_dim,
                &beta, workspace->d_mlp_out, hidden_dim);

    // 11. Residual Add -> x_out
    residual_add_kernel<<<(total_hidden + 255)/256, 256, 0, stream>>>(
        workspace->d_x_norm, workspace->d_mlp_out, d_x_out, total_hidden
    );

    return 0;
}

int blackwell_execute_step(
    const BlackwellResidentModelWeights *model,
    const BlackwellBatchPlanC *plan,
    BlackwellWorkspaceC *workspace,
    uint8_t *kv_pool,
    const KvLayoutDesc *layout,
    uint32_t *h_sampled_tokens,
    cudaStream_t stream,
    cublasHandle_t cublas_handle
) {
    int M = plan->num_tokens;
    if (M <= 0) return 0;
    int R = plan->terminal_count;
    if (!resident_dense_f32(model->d_embed_tokens) ||
        !resident_dense_f32(model->d_final_norm) ||
        !resident_dense_f32(model->d_lm_head)) return -2;

    // Async upload of input embeddings or tokens to persistent GPU workspace
    if (plan->h_input_tokens != NULL && model->d_embed_tokens.address != NULL) {
        cudaMemcpyAsync(workspace->d_tokens, plan->h_input_tokens,
                        (size_t)M * sizeof(uint32_t),
                        cudaMemcpyHostToDevice, stream);
        embedding_lookup_kernel<<<M, 256, 0, stream>>>(
            workspace->d_tokens, dense_f32_data(model->d_embed_tokens), workspace->d_x_a,
            M, model->hidden_dim, model->vocab_size
        );
    } else if (plan->h_input_embeddings != NULL) {
        cudaMemcpyAsync(workspace->d_x_a, plan->h_input_embeddings,
                        (size_t)M * model->hidden_dim * sizeof(float),
                        cudaMemcpyHostToDevice, stream);
    }

    cudaMemcpyAsync(workspace->d_positions, plan->h_positions,
                    (size_t)M * sizeof(int32_t),
                    cudaMemcpyHostToDevice, stream);

    cudaMemcpyAsync(workspace->d_block_ids, plan->h_block_ids,
                    (size_t)M * sizeof(int32_t),
                    cudaMemcpyHostToDevice, stream);

    cudaMemcpyAsync(workspace->d_slots, plan->h_slots,
                    (size_t)M * sizeof(int32_t),
                    cudaMemcpyHostToDevice, stream);

    if (plan->decode_count > 0) {
        cudaMemcpyAsync(workspace->d_block_tables, plan->h_decode_block_tables,
                        (size_t)plan->decode_count * plan->max_blocks_per_seq * sizeof(int32_t),
                        cudaMemcpyHostToDevice, stream);
        cudaMemcpyAsync(workspace->d_context_lens, plan->h_decode_context_lens,
                        (size_t)plan->decode_count * sizeof(int32_t),
                        cudaMemcpyHostToDevice, stream);
    }

    if (plan->prefill_count > 0) {
        cudaMemcpyAsync(workspace->d_prefill_offsets, plan->h_prefill_offsets,
                        (size_t)(plan->prefill_count + 1) * sizeof(int32_t),
                        cudaMemcpyHostToDevice, stream);
    }

    cudaMemcpyAsync(workspace->d_term_indices, plan->h_term_indices,
                    (size_t)R * sizeof(int32_t),
                    cudaMemcpyHostToDevice, stream);

    // Execute all layers sequentially on stream without synchronizing
    for (int l = 0; l < model->num_layers; l++) {
        int layer_status = blackwell_execute_layer(
            l, &model->layers[l], plan, workspace, kv_pool, layout,
            stream, cublas_handle, l % 2,
            model->hidden_dim, model->num_q_heads, model->num_kv_heads,
            model->head_dim, model->intermediate_dim,
            model->rms_norm_eps, model->rope_theta
        );
        if (layer_status != 0) return layer_status;
    }

    float *d_x_final = (model->num_layers % 2 == 1) ? workspace->d_x_b : workspace->d_x_a;

    // Gather terminal rows
    gather_terminal_rows_kernel<<<R, 256, 0, stream>>>(
        d_x_final, workspace->d_term_indices, workspace->d_term_x, R, model->hidden_dim
    );

    // Final RMSNorm on terminal rows
    rmsnorm_batch_kernel<<<R, 256, 0, stream>>>(
        workspace->d_term_x, dense_f32_data(model->d_final_norm), workspace->d_x_norm, R, model->hidden_dim, model->rms_norm_eps
    );

    // LM Head GEMM -> logits [R, vocab_size]
    float alpha = 1.0f, beta = 0.0f;
    cublasSgemm(cublas_handle, CUBLAS_OP_T, CUBLAS_OP_N,
                model->vocab_size, R, model->hidden_dim,
                &alpha, dense_f32_data(model->d_lm_head), model->hidden_dim,
                workspace->d_x_norm, model->hidden_dim,
                &beta, workspace->d_logits, model->vocab_size);

    // Argmax sampling on GPU -> d_tokens [R]
    argmax_sampling_kernel<<<R, 256, 0, stream>>>(
        workspace->d_logits, workspace->d_tokens, R, model->vocab_size
    );

    // Asynchronously copy sampled tokens to host
    cudaMemcpyAsync(h_sampled_tokens, workspace->d_tokens,
                    (size_t)R * sizeof(uint32_t),
                    cudaMemcpyDeviceToHost, stream);

    // EXACTLY ONE COMPLETION FENCE FOR THE ENTIRE SCHEDULER STEP
    cudaError_t err = cudaStreamSynchronize(stream);
    if (err != cudaSuccess) {
        return -1;
    }

    g_kernel_exec_count.fetch_add(1 + (uint64_t)model->num_layers * 11, std::memory_order_relaxed);
    return 0;
}

BlackwellWorkspaceC* blackwell_workspace_create(
    int max_tokens,
    int hidden_dim,
    int q_dim,
    int kv_dim,
    int intermediate_dim,
    int vocab_size,
    int max_blocks_per_seq
) {
    BlackwellWorkspaceC *ws = (BlackwellWorkspaceC*)calloc(1, sizeof(BlackwellWorkspaceC));
    if (!ws) return NULL;

    ws->max_tokens = max_tokens;
    int qkv_dim = q_dim + 2 * kv_dim;

    cudaMalloc((void**)&ws->d_x_a, (size_t)max_tokens * hidden_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_x_b, (size_t)max_tokens * hidden_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_x_norm, (size_t)max_tokens * hidden_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_qkv, (size_t)max_tokens * qkv_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_attn_out, (size_t)max_tokens * q_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_attn_proj, (size_t)max_tokens * hidden_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_post_norm, (size_t)max_tokens * hidden_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_gate_up, (size_t)max_tokens * 2 * intermediate_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_act, (size_t)max_tokens * intermediate_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_mlp_out, (size_t)max_tokens * hidden_dim * sizeof(float));

    cudaMalloc((void**)&ws->d_term_x, (size_t)max_tokens * hidden_dim * sizeof(float));
    cudaMalloc((void**)&ws->d_logits, (size_t)max_tokens * vocab_size * sizeof(float));
    cudaMalloc((void**)&ws->d_tokens, (size_t)max_tokens * sizeof(uint32_t));

    cudaMalloc((void**)&ws->d_positions, (size_t)max_tokens * sizeof(int32_t));
    cudaMalloc((void**)&ws->d_block_ids, (size_t)max_tokens * sizeof(int32_t));
    cudaMalloc((void**)&ws->d_slots, (size_t)max_tokens * sizeof(int32_t));
    cudaMalloc((void**)&ws->d_block_tables, (size_t)max_tokens * max_blocks_per_seq * sizeof(int32_t));
    cudaMalloc((void**)&ws->d_context_lens, (size_t)max_tokens * sizeof(int32_t));
    cudaMalloc((void**)&ws->d_term_indices, (size_t)max_tokens * sizeof(int32_t));
    cudaMalloc((void**)&ws->d_prefill_offsets, (size_t)(max_tokens + 1) * sizeof(int32_t));

    return ws;
}

void blackwell_workspace_free(BlackwellWorkspaceC *ws) {
    if (!ws) return;
    if (ws->d_x_a) cudaFree(ws->d_x_a);
    if (ws->d_x_b) cudaFree(ws->d_x_b);
    if (ws->d_x_norm) cudaFree(ws->d_x_norm);
    if (ws->d_qkv) cudaFree(ws->d_qkv);
    if (ws->d_attn_out) cudaFree(ws->d_attn_out);
    if (ws->d_attn_proj) cudaFree(ws->d_attn_proj);
    if (ws->d_post_norm) cudaFree(ws->d_post_norm);
    if (ws->d_gate_up) cudaFree(ws->d_gate_up);
    if (ws->d_act) cudaFree(ws->d_act);
    if (ws->d_mlp_out) cudaFree(ws->d_mlp_out);
    if (ws->d_term_x) cudaFree(ws->d_term_x);
    if (ws->d_logits) cudaFree(ws->d_logits);
    if (ws->d_tokens) cudaFree(ws->d_tokens);
    if (ws->d_positions) cudaFree(ws->d_positions);
    if (ws->d_block_ids) cudaFree(ws->d_block_ids);
    if (ws->d_slots) cudaFree(ws->d_slots);
    if (ws->d_block_tables) cudaFree(ws->d_block_tables);
    if (ws->d_context_lens) cudaFree(ws->d_context_lens);
    if (ws->d_term_indices) cudaFree(ws->d_term_indices);
    if (ws->d_prefill_offsets) cudaFree(ws->d_prefill_offsets);
    free(ws);
}

BlackwellResidentLayerWeights* blackwell_layer_weights_create(
    const TensorView *h_norm1,
    const TensorView *h_w_qkv,
    const TensorView *h_w_o,
    const TensorView *h_norm2,
    const TensorView *h_w_gate_up,
    const TensorView *h_w_down,
    int hidden_dim,
    int q_dim,
    int kv_dim,
    int intermediate_dim,
    cudaStream_t stream
) {
    int qkv_dim = q_dim + 2 * kv_dim;
    if (!dense_f32_view(h_norm1, 1, hidden_dim) ||
        !dense_f32_view(h_w_qkv, 2, qkv_dim, hidden_dim) ||
        !dense_f32_view(h_w_o, 2, hidden_dim, q_dim) ||
        !dense_f32_view(h_norm2, 1, hidden_dim) ||
        !dense_f32_view(h_w_gate_up, 2, 2 * intermediate_dim, hidden_dim) ||
        !dense_f32_view(h_w_down, 2, hidden_dim, intermediate_dim)) return NULL;
    BlackwellResidentLayerWeights *lw = (BlackwellResidentLayerWeights*)calloc(1, sizeof(BlackwellResidentLayerWeights));
    if (!lw) return NULL;
    if (blackwell_tensor_upload(h_norm1, &lw->d_norm1, stream) != 0 ||
        blackwell_tensor_upload(h_w_qkv, &lw->d_w_qkv, stream) != 0 ||
        blackwell_tensor_upload(h_w_o, &lw->d_w_o, stream) != 0 ||
        blackwell_tensor_upload(h_norm2, &lw->d_norm2, stream) != 0 ||
        blackwell_tensor_upload(h_w_gate_up, &lw->d_w_gate_up, stream) != 0 ||
        blackwell_tensor_upload(h_w_down, &lw->d_w_down, stream) != 0) {
        blackwell_tensor_free(&lw->d_norm1);
        blackwell_tensor_free(&lw->d_w_qkv);
        blackwell_tensor_free(&lw->d_w_o);
        blackwell_tensor_free(&lw->d_norm2);
        blackwell_tensor_free(&lw->d_w_gate_up);
        blackwell_tensor_free(&lw->d_w_down);
        free(lw);
        return NULL;
    }
    return lw;
}

void blackwell_layer_weights_free(BlackwellResidentLayerWeights *lw) {
    if (!lw) return;
    blackwell_tensor_free(&lw->d_norm1);
    blackwell_tensor_free(&lw->d_w_qkv);
    blackwell_tensor_free(&lw->d_w_o);
    blackwell_tensor_free(&lw->d_norm2);
    blackwell_tensor_free(&lw->d_w_gate_up);
    blackwell_tensor_free(&lw->d_w_down);
    free(lw);
}

BlackwellResidentModelWeights* blackwell_model_weights_create(
    int num_layers,
    int hidden_dim,
    int num_q_heads,
    int num_kv_heads,
    int head_dim,
    int intermediate_dim,
    int vocab_size,
    float rms_norm_eps,
    float rope_theta,
    const TensorView *h_embed_tokens,
    const TensorView *h_final_norm,
    const TensorView *h_lm_head,
    cudaStream_t stream
) {
    if (!dense_f32_view(h_embed_tokens, 2, vocab_size, hidden_dim) ||
        !dense_f32_view(h_final_norm, 1, hidden_dim) ||
        !dense_f32_view(h_lm_head, 2, vocab_size, hidden_dim)) return NULL;
    BlackwellResidentModelWeights *mw = (BlackwellResidentModelWeights*)calloc(1, sizeof(BlackwellResidentModelWeights));
    if (!mw) return NULL;

    mw->num_layers = num_layers;
    mw->hidden_dim = hidden_dim;
    mw->num_q_heads = num_q_heads;
    mw->num_kv_heads = num_kv_heads;
    mw->head_dim = head_dim;
    mw->intermediate_dim = intermediate_dim;
    mw->vocab_size = vocab_size;
    mw->rms_norm_eps = rms_norm_eps;
    mw->rope_theta = rope_theta;

    mw->layers = (BlackwellResidentLayerWeights*)calloc(num_layers, sizeof(BlackwellResidentLayerWeights));
    if (!mw->layers ||
        blackwell_tensor_upload(h_embed_tokens, &mw->d_embed_tokens, stream) != 0 ||
        blackwell_tensor_upload(h_final_norm, &mw->d_final_norm, stream) != 0 ||
        blackwell_tensor_upload(h_lm_head, &mw->d_lm_head, stream) != 0) {
        blackwell_tensor_free(&mw->d_embed_tokens);
        blackwell_tensor_free(&mw->d_final_norm);
        blackwell_tensor_free(&mw->d_lm_head);
        free(mw->layers);
        free(mw);
        return NULL;
    }
    return mw;
}

void blackwell_model_weights_set_layer(
    BlackwellResidentModelWeights *mw,
    int layer_idx,
    const BlackwellResidentLayerWeights *lw
) {
    if (mw && lw && layer_idx >= 0 && layer_idx < mw->num_layers) {
        mw->layers[layer_idx] = *lw;
        free((void *)lw);
    }
}

void blackwell_model_weights_free(BlackwellResidentModelWeights *mw) {
    if (!mw) return;
    if (mw->layers) {
        for (int i = 0; i < mw->num_layers; i++) {
            blackwell_tensor_free(&mw->layers[i].d_norm1);
            blackwell_tensor_free(&mw->layers[i].d_w_qkv);
            blackwell_tensor_free(&mw->layers[i].d_w_o);
            blackwell_tensor_free(&mw->layers[i].d_norm2);
            blackwell_tensor_free(&mw->layers[i].d_w_gate_up);
            blackwell_tensor_free(&mw->layers[i].d_w_down);
        }
        free(mw->layers);
    }
    blackwell_tensor_free(&mw->d_embed_tokens);
    blackwell_tensor_free(&mw->d_final_norm);
    blackwell_tensor_free(&mw->d_lm_head);
    free(mw);
}

} // extern "C"
