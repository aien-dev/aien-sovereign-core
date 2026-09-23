#pragma once

#include <cuda_runtime.h>
#include <stdint.h>

// Numeric tags mirror src/tensor_abi.rs. Keep this layout stable across FFI.
enum TensorDType : uint32_t {
    TENSOR_F32 = 1, TENSOR_BF16 = 2, TENSOR_FP16 = 3,
    TENSOR_FP8_E4M3FN = 4, TENSOR_FP8_E5M2 = 5,
    TENSOR_INT8 = 6, TENSOR_INT4 = 7,
};
enum TensorLayout : uint32_t {
    TENSOR_ROW_MAJOR = 1, TENSOR_COLUMN_MAJOR = 2, TENSOR_STRIDED = 3,
};
enum TensorResidency : uint32_t { TENSOR_HOST = 1, TENSOR_DEVICE = 2 };
enum QuantScheme : uint32_t { QUANT_NONE = 0, QUANT_SYMMETRIC = 1, QUANT_AFFINE = 2 };
enum TensorPacking : uint32_t {
    PACKING_NONE = 0, INT4_LOW_SIGNED = 1, INT4_HIGH_SIGNED = 2,
    INT4_LOW_UNSIGNED = 3, INT4_HIGH_UNSIGNED = 4,
};

struct AbiQuantization {
    QuantScheme scheme;
    int32_t axis;
    uint32_t group_size;
    TensorPacking packing;
    const float *scales;
    uint64_t scale_count;
    const int32_t *zero_points;
    uint64_t zero_point_count;
};

struct TensorDescriptor {
    TensorDType dtype;
    TensorLayout layout;
    uint32_t rank;
    TensorResidency residency;
    uint64_t shape[4];
    uint64_t strides[4];
    uint64_t byte_len;
    AbiQuantization quant;
};

struct TensorView {
    const void *address;
    TensorDescriptor descriptor;
};

struct ResidentTensor {
    void *address;
    TensorDescriptor descriptor;
};

static_assert(sizeof(AbiQuantization) == 48, "quant ABI drift");
static_assert(sizeof(TensorDescriptor) == 136, "tensor descriptor ABI drift");
static_assert(sizeof(TensorView) == 144, "tensor view ABI drift");
static_assert(sizeof(ResidentTensor) == 144, "resident tensor ABI drift");

extern "C" int blackwell_tensor_upload(
    const TensorView *host, ResidentTensor *device, cudaStream_t stream);
extern "C" void blackwell_tensor_free(ResidentTensor *device);
