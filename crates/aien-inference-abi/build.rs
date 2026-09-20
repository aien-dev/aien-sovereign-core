use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_blackwell_cuda)");
    println!("cargo:rerun-if-changed=cuda/blackwell_gemm.cu");
    println!("cargo:rerun-if-changed=cuda/paged_attention_bf16.cu");
    println!("cargo:rerun-if-changed=cuda/blackwell_layer.cu");
    println!("cargo:rerun-if-changed=build.rs");

    let nvcc = PathBuf::from("/usr/local/cuda/bin/nvcc");
    if !nvcc.exists() {
        println!("cargo:warning=nvcc not found at /usr/local/cuda/bin/nvcc; Blackwell CUDA backend disabled.");
        return;
    }

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = Path::new(&out_dir);
    let gemm_obj_path = out_path.join("blackwell_gemm.o");
    let attn_obj_path = out_path.join("paged_attention_bf16.o");
    let layer_obj_path = out_path.join("blackwell_layer.o");
    let lib_path = out_path.join("libblackwell_gemm.a");

    let status = Command::new(&nvcc)
        .args([
            "-c",
            "-O3",
            "-arch=sm_121",
            "-Xcompiler",
            "-fPIC",
            "cuda/blackwell_gemm.cu",
            "-o",
            gemm_obj_path.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute nvcc for blackwell_gemm.cu");

    if !status.success() {
        panic!("nvcc compilation of cuda/blackwell_gemm.cu failed");
    }

    let status2 = Command::new(&nvcc)
        .args([
            "-c",
            "-O3",
            "-arch=sm_121",
            "-Xcompiler",
            "-fPIC",
            "cuda/paged_attention_bf16.cu",
            "-o",
            attn_obj_path.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute nvcc for paged_attention_bf16.cu");

    if !status2.success() {
        panic!("nvcc compilation of cuda/paged_attention_bf16.cu failed");
    }

    let status3 = Command::new(&nvcc)
        .args([
            "-c",
            "-O3",
            "-arch=sm_121",
            "-Xcompiler",
            "-fPIC",
            "cuda/blackwell_layer.cu",
            "-o",
            layer_obj_path.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute nvcc for blackwell_layer.cu");

    if !status3.success() {
        panic!("nvcc compilation of cuda/blackwell_layer.cu failed");
    }

    let ar_status = Command::new("ar")
        .args([
            "crs",
            lib_path.to_str().unwrap(),
            gemm_obj_path.to_str().unwrap(),
            attn_obj_path.to_str().unwrap(),
            layer_obj_path.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute ar");

    if !ar_status.success() {
        panic!("ar archive creation failed");
    }

    println!("cargo:rustc-link-search=native={}", out_dir);
    println!("cargo:rustc-link-lib=static=blackwell_gemm");
    println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
    println!("cargo:rustc-link-lib=dylib=cublas");
    println!("cargo:rustc-link-lib=dylib=cudart");
    println!("cargo:rustc-link-lib=dylib=stdc++");
    println!("cargo:rustc-cfg=has_blackwell_cuda");
}
