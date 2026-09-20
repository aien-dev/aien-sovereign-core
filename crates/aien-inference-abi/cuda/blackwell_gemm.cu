#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unordered_map>
#include <mutex>
#include <atomic>
#include <cuda_runtime.h>
#include <cublas_v2.h>

static cublasHandle_t g_cublas_handle = NULL;
static cudaStream_t g_cuda_stream = NULL;
static std::recursive_mutex g_backend_mutex;
std::atomic<uint64_t> g_kernel_exec_count{0};

// Pre-allocated scratch activation buffers on GB10 device
static float *g_d_a = NULL;
static size_t g_cap_a = 0;
static float *g_d_c = NULL;
static size_t g_cap_c = 0;

// Device cache for model weight tensors to eliminate per-token HtoD copies
static std::unordered_map<const float*, float*> g_weight_cache;

extern "C" {

int blackwell_gemm_init(void);

void* blackwell_get_stream(void) { if (!g_cuda_stream) blackwell_gemm_init(); return (void*)g_cuda_stream; }
void* blackwell_get_cublas_handle(void) { if (!g_cublas_handle) blackwell_gemm_init(); return (void*)g_cublas_handle; }

int blackwell_gemm_init(void) {
    std::lock_guard<std::recursive_mutex> lock(g_backend_mutex);
    if (g_cublas_handle != NULL) {
        return 0;
    }

    cudaError_t cuda_err = cudaSetDevice(0);
    if (cuda_err != cudaSuccess) {
        fprintf(stderr, "BlackwellGb10Backend init: cudaSetDevice(0) failed (%d: %s); clear unified memory pressure via vm.drop_caches=3\n", (int)cuda_err, cudaGetErrorString(cuda_err));
        return -1;
    }

    cuda_err = cudaStreamCreateWithFlags(&g_cuda_stream, cudaStreamNonBlocking);
    if (cuda_err != cudaSuccess) {
        return -2;
    }

    cublasStatus_t cublas_stat = cublasCreate(&g_cublas_handle);
    if (cublas_stat != CUBLAS_STATUS_SUCCESS) {
        return -3;
    }

    cublas_stat = cublasSetStream(g_cublas_handle, g_cuda_stream);
    if (cublas_stat != CUBLAS_STATUS_SUCCESS) {
        return -4;
    }

    // Pre-allocate initial 16MB scratch buffers for activations
    g_cap_a = 2048 * 2048;
    cudaMalloc(&g_d_a, g_cap_a * sizeof(float));

    g_cap_c = 2048 * 2048;
    cudaMalloc(&g_d_c, g_cap_c * sizeof(float));

    return 0;
}

uint64_t blackwell_gemm_get_kernel_count(void) {
    return g_kernel_exec_count.load(std::memory_order_relaxed);
}

int blackwell_gemm_get_device_name(char *buf, int max_len) {
    if (!buf || max_len <= 0) return -1;
    cudaDeviceProp prop;
    cudaError_t err = cudaGetDeviceProperties(&prop, 0);
    if (err != cudaSuccess) return -2;
    strncpy(buf, prop.name, max_len - 1);
    buf[max_len - 1] = 0;
    return 0;
}

static float* internal_get_cached_weight(const float *h_w, size_t count) {
    auto it = g_weight_cache.find(h_w);
    if (it != g_weight_cache.end()) {
        return it->second;
    }

    float *d_w = NULL;
    cudaError_t err = cudaMalloc(&d_w, count * sizeof(float));
    if (err != cudaSuccess) {
        return NULL;
    }

    err = cudaMemcpyAsync(d_w, h_w, count * sizeof(float), cudaMemcpyHostToDevice, g_cuda_stream);
    if (err != cudaSuccess) {
        cudaFree(d_w);
        return NULL;
    }

    g_weight_cache[h_w] = d_w;
    return d_w;
}

static float* internal_ensure_scratch_a(size_t count) {
    if (count > g_cap_a) {
        if (g_d_a) cudaFree(g_d_a);
        if (cudaMalloc(&g_d_a, count * sizeof(float)) != cudaSuccess) {
            g_cap_a = 0;
            return NULL;
        }
        g_cap_a = count;
    }
    return g_d_a;
}

static float* internal_ensure_scratch_c(size_t count) {
    if (count > g_cap_c) {
        if (g_d_c) cudaFree(g_d_c);
        if (cudaMalloc(&g_d_c, count * sizeof(float)) != cudaSuccess) {
            g_cap_c = 0;
            return NULL;
        }
        g_cap_c = count;
    }
    return g_d_c;
}

int blackwell_gemm_f32(const float *a, const float *b, float *c, int m, int k, int n) {
    if (!a || !b || !c || m <= 0 || k <= 0 || n <= 0) {
        return -1;
    }

    std::lock_guard<std::recursive_mutex> lock(g_backend_mutex);
    if (g_cublas_handle == NULL) {
        if (blackwell_gemm_init() != 0) {
            return -2;
        }
    }

    float *d_a = internal_ensure_scratch_a((size_t)m * k);
    float *d_c = internal_ensure_scratch_c((size_t)m * n);
    float *d_b = internal_get_cached_weight(b, (size_t)n * k);

    if (!d_a || !d_b || !d_c) {
        return -3;
    }

    cudaError_t err = cudaMemcpyAsync(d_a, a, (size_t)m * k * sizeof(float), cudaMemcpyHostToDevice, g_cuda_stream);
    if (err != cudaSuccess) {
        return -4;
    }

    float alpha = 1.0f;
    float beta = 0.0f;

    // Row-major: C [m, n] = A [m, k] * B^T [k, n] (where B is row-major [n, k])
    // Col-major equivalent: C^T [n, m] = B [n, k] * A^T [k, m]
    // B as col-major is (k, n), op(B) = CUBLAS_OP_T -> (n, k)
    // A as col-major is (k, m), op(A) = CUBLAS_OP_N -> (k, m)
    cublasStatus_t stat = cublasSgemm(
        g_cublas_handle,
        CUBLAS_OP_T,
        CUBLAS_OP_N,
        n, m, k,
        &alpha,
        d_b, k,
        d_a, k,
        &beta,
        d_c, n
    );

    if (stat != CUBLAS_STATUS_SUCCESS) {
        return -5;
    }

    err = cudaMemcpyAsync(c, d_c, (size_t)m * n * sizeof(float), cudaMemcpyDeviceToHost, g_cuda_stream);
    if (err != cudaSuccess) {
        return -6;
    }

    err = cudaStreamSynchronize(g_cuda_stream);
    if (err != cudaSuccess) {
        return -7;
    }

    g_kernel_exec_count.fetch_add(1, std::memory_order_relaxed);
    return 0;
}

int blackwell_gemv_f32(const float *x, const float *weight, float *out, int in_dim, int out_dim) {
    return blackwell_gemm_f32(x, weight, out, 1, in_dim, out_dim);
}

void* blackwell_allocate_managed(size_t bytes) {
    void* ptr = NULL;
    cudaError_t err = cudaMallocManaged(&ptr, bytes, cudaMemAttachGlobal);
    if (err != cudaSuccess) {
        return NULL;
    }
    return ptr;
}

void blackwell_free_managed(void* ptr) {
    if (ptr != NULL) {
        cudaFree(ptr);
    }
}

void blackwell_gemm_destroy(void) {
    std::lock_guard<std::recursive_mutex> lock(g_backend_mutex);
    for (auto &pair : g_weight_cache) {
        if (pair.second) cudaFree(pair.second);
    }
    g_weight_cache.clear();

    if (g_d_a) { cudaFree(g_d_a); g_d_a = NULL; g_cap_a = 0; }
    if (g_d_c) { cudaFree(g_d_c); g_d_c = NULL; g_cap_c = 0; }
    if (g_cublas_handle) { cublasDestroy(g_cublas_handle); g_cublas_handle = NULL; }
    if (g_cuda_stream) { cudaStreamDestroy(g_cuda_stream); g_cuda_stream = NULL; }
}

}
