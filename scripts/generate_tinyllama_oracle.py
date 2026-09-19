#!/usr/bin/env python3
"""Deterministic Reference Oracle Fixture Generator for TinyLlama-1.1B-Chat-v1.0.

Executes TinyLlama on CPU in FP32 using Hugging Face Transformers to generate
canonical reference activation fixtures, logits, and multi-step greedy generation
sequences for numerical parity verification.
"""

import argparse
import hashlib
import json
import os
import shutil
import sys
from typing import Any, Dict, List, Tuple

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer
from transformers.models.llama import modeling_llama
import safetensors.torch


PINNED_SNAPSHOT = (
    "/home/drakestapleton/.cache/huggingface/hub/"
    "models--TinyLlama--TinyLlama-1.1B-Chat-v1.0/snapshots/"
    "fe8a4ea1ffedaf415f4da2f062534de366a451e6"
)

CANONICAL_PROMPT = (
    "<|system|>\n"
    "You are a sovereign AI assistant.</s>\n"
    "<|user|>\n"
    "Explain the role of an operating system in one sentence.</s>\n"
    "<|assistant|>\n"
)

EXPECTED_PROMPT_TOKENS = [
    1, 529, 29989, 5205, 29989, 29958, 13, 3492, 526, 263, 577, 369, 7577, 319,
    29902, 20255, 29889, 2, 13, 29966, 29989, 1792, 29989, 29958, 13, 9544,
    7420, 278, 6297, 310, 385, 13598, 1788, 297, 697, 10541, 29889, 2, 13,
    29966, 29989, 465, 22137, 29989, 29958, 13
]

EXPECTED_GREEDY_16 = [
    2744, 13598, 1788, 313, 3267, 29897, 338, 263, 7047, 393, 767, 1179,
    278, 12837, 322, 7047
]

EXPECTED_DECODED_TEXT = (
    "An operating system (OS) is a software that manages the hardware and software"
)


def compute_file_sha256(path: str) -> str:
    """Compute hex-encoded SHA-256 digest of a file."""
    hasher = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(1024 * 1024):
            hasher.update(chunk)
    return hasher.hexdigest()


def capture_oracle(
    model_path: str,
    output_dir: str,
    num_steps: int = 16,
) -> Tuple[str, str]:
    """Execute reference forward passes and export oracle fixtures."""
    os.makedirs(output_dir, exist_ok=True)

    print(f"Loading tokenizer from: {model_path}")
    tokenizer = AutoTokenizer.from_pretrained(model_path)

    print(f"Loading model on CPU in FP32 from: {model_path}")
    model = AutoModelForCausalLM.from_pretrained(
        model_path,
        dtype=torch.float32,
        device_map="cpu",
        attn_implementation="eager",
    )
    model.eval()

    # 1. Validate prompt tokenization
    input_ids = tokenizer.encode(CANONICAL_PROMPT, return_tensors="pt")
    prompt_tokens = input_ids[0].tolist()

    if prompt_tokens != EXPECTED_PROMPT_TOKENS:
        raise ValueError(
            f"Prompt token ID mismatch:\nExpected: {EXPECTED_PROMPT_TOKENS}\nActual:   {prompt_tokens}"
        )
    print(f"Tokenization validated: {len(prompt_tokens)} tokens matching golden spec.")

    # 2. Setup activation capture dictionaries
    activations: Dict[str, torch.Tensor] = {}
    current_layer_idx = [-1]

    # Intercept RoPE via monkey-patch
    orig_apply_rotary = modeling_llama.apply_rotary_pos_emb

    def hooked_apply_rotary(q, k, cos, sin, unsqueeze_dim=1):
        q_rot, k_rot = orig_apply_rotary(q, k, cos, sin, unsqueeze_dim=unsqueeze_dim)
        layer_idx = current_layer_idx[0]
        if layer_idx >= 0:
            activations[f"layers.{layer_idx}.post_rope_q"] = (
                q_rot.detach().clone().contiguous()
            )
            activations[f"layers.{layer_idx}.post_rope_k"] = (
                k_rot.detach().clone().contiguous()
            )
        return q_rot, k_rot

    modeling_llama.apply_rotary_pos_emb = hooked_apply_rotary

    # Intercept layer operations
    layer_in_states: Dict[int, torch.Tensor] = {}
    attn_out_states: Dict[int, torch.Tensor] = {}

    def make_layer_pre_hook(i: int):
        def hook(mod, inp):
            current_layer_idx[0] = i
            layer_in_states[i] = inp[0].detach().clone()
        return hook

    def make_input_norm_hook(i: int):
        def hook(mod, inp, out):
            activations[f"layers.{i}.post_rmsnorm"] = (
                out.detach().clone().contiguous()
            )
        return hook

    def make_attn_o_hook(i: int):
        def hook(mod, inp, out):
            attn_out_states[i] = out.detach().clone()
            activations[f"layers.{i}.post_attention"] = (
                out.detach().clone().contiguous()
            )
        return hook

    def make_post_attn_norm_hook(i: int):
        def hook(mod, inp, out):
            activations[f"layers.{i}.post_attn_norm"] = (
                out.detach().clone().contiguous()
            )
        return hook

    def make_down_proj_hook(i: int):
        def hook(mod, inp, out):
            activations[f"layers.{i}.post_swiglu"] = (
                out.detach().clone().contiguous()
            )
        return hook

    def make_layer_post_hook(i: int):
        def hook(mod, inp, out):
            attn_res = layer_in_states[i] + attn_out_states[i]
            activations[f"layers.{i}.post_attn_residual"] = (
                attn_res.detach().clone().contiguous()
            )
            activations[f"layers.{i}.post_residual"] = (
                out.detach().clone().contiguous()
            )
        return hook

    hooks = []
    for i, layer in enumerate(model.model.layers):
        hooks.append(layer.register_forward_pre_hook(make_layer_pre_hook(i)))
        hooks.append(layer.input_layernorm.register_forward_hook(make_input_norm_hook(i)))
        hooks.append(layer.self_attn.o_proj.register_forward_hook(make_attn_o_hook(i)))
        hooks.append(layer.post_attention_layernorm.register_forward_hook(make_post_attn_norm_hook(i)))
        hooks.append(layer.mlp.down_proj.register_forward_hook(make_down_proj_hook(i)))
        hooks.append(layer.register_forward_hook(make_layer_post_hook(i)))

    # Global embedding, final norm, and logits hooks
    def embed_hook(mod, inp, out):
        activations["embed_tokens"] = out.detach().clone().contiguous()

    def norm_hook(mod, inp, out):
        activations["final_norm"] = out.detach().clone().contiguous()

    def lm_head_hook(mod, inp, out):
        activations["logits"] = out.detach().clone().contiguous()
        activations["last_token_logits"] = out[0, -1, :].detach().clone().contiguous()

    hooks.append(model.model.embed_tokens.register_forward_hook(embed_hook))
    hooks.append(model.model.norm.register_forward_hook(norm_hook))
    hooks.append(model.lm_head.register_forward_hook(lm_head_hook))

    # 3. Execute Golden Prompt Forward Pass
    print("Executing single-forward pass on prompt tokens...")
    with torch.no_grad():
        outputs = model(input_ids)

    # Clean up forward hooks
    for h in hooks:
        h.remove()
    modeling_llama.apply_rotary_pos_emb = orig_apply_rotary

    logits = outputs.logits
    last_token_logits = logits[0, -1, :]

    # 4. Compute Top-10 Logits and Next Token
    top10_vals, top10_inds = torch.topk(last_token_logits, 10)
    top10_token_ids = top10_inds.tolist()
    top10_logit_values = [float(v) for v in top10_vals.tolist()]

    greedy_next_token_id = int(top10_token_ids[0])
    greedy_next_token_logit = float(top10_logit_values[0])
    greedy_next_token_text = tokenizer.decode([greedy_next_token_id])

    print(f"Greedy next token: ID={greedy_next_token_id} ({repr(greedy_next_token_text)}), logit={greedy_next_token_logit:.4f}")
    if greedy_next_token_id != 2744:
        raise ValueError(f"Greedy next token mismatch: expected 2744, got {greedy_next_token_id}")

    top_tokens_meta = []
    for idx, logit in zip(top10_token_ids, top10_logit_values):
        tok_str = tokenizer.decode([idx])
        top_tokens_meta.append({
            "token_id": idx,
            "token_str": tok_str,
            "logit": logit,
        })
        print(f"  Rank {len(top_tokens_meta)}: ID {idx:>5} ({repr(tok_str):<10}) -> logit {logit:.4f}")

    # 5. Multi-Step Autoregressive Greedy Decode
    print(f"Executing {num_steps}-step autoregressive greedy generation...")
    curr_ids = input_ids.clone()
    generated_tokens: List[int] = []
    generated_pieces: List[str] = []

    with torch.no_grad():
        for step in range(num_steps):
            step_out = model(curr_ids)
            next_tok = torch.argmax(step_out.logits[:, -1, :], dim=-1)
            tok_id = int(next_tok.item())
            generated_tokens.append(tok_id)
            generated_pieces.append(tokenizer.decode([tok_id]))
            curr_ids = torch.cat([curr_ids, next_tok.unsqueeze(0)], dim=-1)

    decoded_text = tokenizer.decode(generated_tokens)
    print(f"Generated tokens: {generated_tokens}")
    print(f"Decoded text: {repr(decoded_text)}")

    if generated_tokens != EXPECTED_GREEDY_16:
        raise ValueError(
            f"16-step token ID sequence mismatch:\nExpected: {EXPECTED_GREEDY_16}\nActual:   {generated_tokens}"
        )
    if decoded_text != EXPECTED_DECODED_TEXT:
        raise ValueError(
            f"Decoded text mismatch:\nExpected: {repr(EXPECTED_DECODED_TEXT)}\nActual:   {repr(decoded_text)}"
        )

    # 6. Save Safetensors Fixture
    # Ensure every tensor is contiguous and detached
    contiguous_activations = {
        name: tensor.detach().clone().contiguous()
        for name, tensor in activations.items()
    }

    safetensors_path = os.path.join(output_dir, "tinyllama_oracle.safetensors")
    print(f"Saving {len(contiguous_activations)} oracle tensors to {safetensors_path}...")
    safetensors.torch.save_file(contiguous_activations, safetensors_path)
    safetensors_sha256 = compute_file_sha256(safetensors_path)
    safetensors_bytes = os.path.getsize(safetensors_path)
    print(f"Safetensors fixture saved: {safetensors_bytes} bytes, SHA-256: {safetensors_sha256}")

    # 7. Copy Model Config and Tokenizer to Fixtures
    tokenizer_src = os.path.join(model_path, "tokenizer.json")
    tokenizer_dst = os.path.join(output_dir, "tokenizer.json")
    shutil.copy2(tokenizer_src, tokenizer_dst)
    tokenizer_sha256 = compute_file_sha256(tokenizer_dst)

    config_src = os.path.join(model_path, "config.json")
    if os.path.exists(config_src):
        shutil.copy2(config_src, os.path.join(output_dir, "config.json"))

    tok_config_src = os.path.join(model_path, "tokenizer_config.json")
    if os.path.exists(tok_config_src):
        shutil.copy2(tok_config_src, os.path.join(output_dir, "tokenizer_config.json"))

    model_safetensors_path = os.path.join(model_path, "model.safetensors")
    model_sha256 = compute_file_sha256(model_safetensors_path) if os.path.exists(model_safetensors_path) else "unknown"

    # 8. Build JSON Manifest
    tensor_catalog: Dict[str, Dict[str, Any]] = {}
    for name, tensor in sorted(contiguous_activations.items()):
        tensor_catalog[name] = {
            "shape": list(tensor.shape),
            "dtype": str(tensor.dtype).replace("torch.", "").upper(),
            "numel": tensor.numel(),
        }

    manifest = {
        "model_id": "TinyLlama/TinyLlama-1.1B-Chat-v1.0",
        "model_sha256": model_sha256,
        "tokenizer_sha256": tokenizer_sha256,
        "safetensors_file": "tinyllama_oracle.safetensors",
        "safetensors_sha256": safetensors_sha256,
        "safetensors_bytes": safetensors_bytes,
        "prompt_text": CANONICAL_PROMPT,
        "prompt_tokens": prompt_tokens,
        "prompt_token_count": len(prompt_tokens),
        "greedy_next_token_id": greedy_next_token_id,
        "greedy_next_token_text": greedy_next_token_text,
        "greedy_next_token_logit": greedy_next_token_logit,
        "top_10_tokens": top_tokens_meta,
        "top_10_token_ids": top10_token_ids,
        "top_10_logits": top10_logit_values,
        "decoded_16_steps": {
            "token_ids": generated_tokens,
            "token_strings": generated_pieces,
            "text": decoded_text,
        },
        "tolerances": {
            "abs_tol": 1e-4,
            "rel_tol": 1e-4,
            "min_cosine": 0.9999,
        },
        "tensor_count": len(tensor_catalog),
        "tensors": tensor_catalog,
    }

    manifest_path = os.path.join(output_dir, "tinyllama_oracle_manifest.json")
    with open(manifest_path, "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)
    manifest_sha256 = compute_file_sha256(manifest_path)
    print(f"Manifest saved to {manifest_path}, SHA-256: {manifest_sha256}")

    return safetensors_path, manifest_path


def main():
    parser = argparse.ArgumentParser(
        description="Generate TinyLlama FP32 reference oracle fixtures on Spark."
    )
    parser.add_argument(
        "--model-path",
        type=str,
        default=PINNED_SNAPSHOT,
        help="Path to Hugging Face snapshot directory.",
    )
    parser.add_argument(
        "--output-dir",
        type=str,
        default="crates/aien-inference-abi/fixtures",
        help="Output directory for generated fixtures.",
    )
    parser.add_argument(
        "--steps",
        type=int,
        default=16,
        help="Number of greedy decode steps.",
    )

    args = parser.parse_args()

    if not os.path.exists(args.model_path):
        print(f"Error: model path does not exist: {args.model_path}", file=sys.stderr)
        sys.exit(1)

    safetensors_path, manifest_path = capture_oracle(
        model_path=args.model_path,
        output_dir=args.output_dir,
        num_steps=args.steps,
    )
    print("Oracle generation completed successfully.")
    print(f"Safetensors: {safetensors_path}")
    print(f"Manifest:    {manifest_path}")


if __name__ == "__main__":
    main()
