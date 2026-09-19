use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=cuda/blackwell_gemm.cu");
    println!("cargo:rerun-if-changed=build.rs");

    let nvcc = PathBuf::from("/usr/local/cuda/bin/nvcc");
    if !nvcc.exists() {
        println!("cargo:warning=nvcc not found at /usr/local/cuda/bin/nvcc; Blackwell CUDA backend disabled.");
        return;
    }

    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = Path::new(&out_dir);
    let obj_path = out_path.join("blackwell_gemm.o");
    let lib_path = out_path.join("libblackwell_gemm.a");

    let cuda_src = "cuda/blackwell_gemm.cu";

    let status = Command::new(&nvcc)
        .args([
            "-c",
            "-O3",
            "-arch=sm_121",
            "-Xcompiler",
            "-fPIC",
            cuda_src,
            "-o",
            obj_path.to_str().unwrap(),
        ])
        .status()
        .expect("Failed to execute nvcc");

    if !status.success() {
        panic!("nvcc compilation of {} failed", cuda_src);
    }

    let ar_status = Command::new("ar")
        .args([
            "crs",
            lib_path.to_str().unwrap(),
            obj_path.to_str().unwrap(),
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
