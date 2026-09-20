fn main() {
    println!("cargo:rustc-check-cfg=cfg(has_blackwell_cuda)");
    let nvcc = std::path::PathBuf::from("/usr/local/cuda/bin/nvcc");
    if nvcc.exists() {
        println!("cargo:rustc-link-search=native=/usr/local/cuda/lib64");
        println!("cargo:rustc-link-lib=dylib=cudart");
        println!("cargo:rustc-cfg=has_blackwell_cuda");
    }
}
