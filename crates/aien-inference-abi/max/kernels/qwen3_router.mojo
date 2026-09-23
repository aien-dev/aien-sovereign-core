import extensibility
from extensibility import InputTensor, OutputTensor, ManagedTensorSlice
from std.gpu import block_idx, thread_idx
from max.gpu.host import DeviceContext
from std.math import exp
from std.collections import InlineArray
from std.utils.index import IndexList

def _route_gpu(
    expert_ids: ManagedTensorSlice[dtype=DType.int32, rank=2, mut=True, ...],
    weights: ManagedTensorSlice[dtype=DType.float32, rank=2, mut=True, ...],
    logits: ManagedTensorSlice[rank=2, ...],
    ctx: DeviceContext,
) raises:
    var num_tokens = logits.dim_size(0)
    var num_experts = logits.dim_size(1)
    if num_experts != 128 or expert_ids.dim_size(1) != 8:
        raise Error("Qwen3-Coder-A3B router requires 128 experts and top 8")

    @parameter
    def route_kernel(token_count: Int32):
            var token = block_idx.x * 128 + thread_idx.x
            if token >= Int(token_count):
                return
            var selected = InlineArray[Int32, 8](fill=-1)
            var selected_logits = InlineArray[Float32, 8](fill=0.0)
            for slot in range(8):
                var best_expert: Int32 = -1
                var best_logit = Float32(-1.0e30)
                for expert in range(128):
                    var seen = False
                    for previous in range(slot):
                        if selected[previous] == Int32(expert):
                            seen = True
                    if not seen:
                        var value = Float32(logits.load[1](IndexList[2](token, expert))[0])
                        if value > best_logit:
                            best_logit = value
                            best_expert = Int32(expert)
                selected[slot] = best_expert
                selected_logits[slot] = best_logit
            var denominator = Float32(0.0)
            for slot in range(8):
                denominator += exp(selected_logits[slot] - selected_logits[0])
            for slot in range(8):
                expert_ids.store[1](IndexList[2](token, slot), SIMD[DType.int32, 1](selected[slot]))
                weights.store[1](IndexList[2](token, slot), SIMD[DType.float32, 1](exp(selected_logits[slot] - selected_logits[0]) / denominator))

    ctx.enqueue_function[route_kernel](
        Int32(num_tokens),
        grid_dim=(num_tokens + 127) // 128,
        block_dim=128,
    )


# Each GPU lane routes one token. This keeps routing local to the device and
# never materializes router logits or assignment lists on the Rust host.
@extensibility.register("aien.qwen3_moe.route_top8")
struct Qwen3RouteTop8:
    @staticmethod
    def execute[target: StaticString](
        expert_ids: OutputTensor[dtype=DType.int32, rank=2, ...],
        weights: OutputTensor[dtype=DType.float32, rank=2, ...],
        logits: InputTensor[rank=2, ...],
        ctx: DeviceContext,
    ) raises:
        comptime if target == "gpu":
            _route_gpu(expert_ids, weights, logits, ctx)
        else:
            raise Error("Qwen3-Coder-A3B routing requires a GPU")
