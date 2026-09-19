use libloading::{Library, Symbol};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn default_so_path() -> PathBuf {
    if let Ok(p) = std::env::var("SPARK_MAX_SO_PATH") {
        return PathBuf::from(p);
    }
    if let Some(p) = option_env!("SPARK_MAX_SO_BUILT") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return pb;
        }
    }
    let local = PathBuf::from("crates/spark-max-cabi/mojo/libspark_max.so");
    if local.exists() {
        return local;
    }
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let candidate = PathBuf::from(manifest).join("mojo/libspark_max.so");
        if candidate.exists() {
            return candidate;
        }
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    home.join("workspace/aien-sovereign-core/crates/spark-max-cabi/mojo/libspark_max.so")
}

pub type FnVersion = unsafe extern "C" fn() -> i32;
pub type FnSessionCreate = unsafe extern "C" fn(i32) -> i32;
pub type FnSessionDestroy = unsafe extern "C" fn(i32) -> i32;
pub type FnComputeScalar = unsafe extern "C" fn(i32, f32) -> f32;
pub type FnLoadModel = unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32) -> i32;
pub type FnForwardPrefill = unsafe extern "C" fn(i32, i32, i32, i32) -> f32;
pub type FnForwardDecode = unsafe extern "C" fn(i32, i32, i32) -> f32;
pub type FnSampleToken = unsafe extern "C" fn(i32, i64, i32) -> i32;
pub type FnKvPoolRegister = unsafe extern "C" fn(i32, i64, i64, i32, i32) -> i32;
pub type FnStepExecute = unsafe extern "C" fn(i32, i32, i32, i32, i64, i64, i64, i64, i64) -> f32;

pub struct SparkMaxBindings {
    _lib: Library,
    pub version: FnVersion,
    pub session_create: FnSessionCreate,
    pub session_destroy: FnSessionDestroy,
    pub compute_scalar: FnComputeScalar,
    pub load_model: FnLoadModel,
    pub forward_prefill: FnForwardPrefill,
    pub forward_decode: FnForwardDecode,
    pub sample_token: FnSampleToken,
    pub kv_pool_register: FnKvPoolRegister,
    pub step_execute: FnStepExecute,
}

static BINDINGS: OnceLock<Result<SparkMaxBindings, String>> = OnceLock::new();

impl SparkMaxBindings {
    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path_ref = path.as_ref();
        if !path_ref.exists() {
            return Err(format!("Library does not exist at {:?}", path_ref));
        }

        unsafe {
            let lib = Library::new(path_ref)
                .map_err(|e| format!("Failed to dlopen {:?}: {}", path_ref, e))?;

            let version: Symbol<FnVersion> = lib
                .get(b"spark_max_version\0")
                .map_err(|e| format!("Symbol spark_max_version not found: {}", e))?;
            let session_create: Symbol<FnSessionCreate> = lib
                .get(b"spark_max_session_create\0")
                .map_err(|e| format!("Symbol spark_max_session_create not found: {}", e))?;
            let session_destroy: Symbol<FnSessionDestroy> = lib
                .get(b"spark_max_session_destroy\0")
                .map_err(|e| format!("Symbol spark_max_session_destroy not found: {}", e))?;
            let compute_scalar: Symbol<FnComputeScalar> = lib
                .get(b"spark_max_compute_scalar\0")
                .map_err(|e| format!("Symbol spark_max_compute_scalar not found: {}", e))?;

            let load_model: Symbol<FnLoadModel> = lib
                .get(b"spark_max_load_model\0")
                .map_err(|e| format!("Symbol spark_max_load_model not found: {}", e))?;
            let forward_prefill: Symbol<FnForwardPrefill> = lib
                .get(b"spark_max_forward_prefill\0")
                .map_err(|e| format!("Symbol spark_max_forward_prefill not found: {}", e))?;
            let forward_decode: Symbol<FnForwardDecode> = lib
                .get(b"spark_max_forward_decode\0")
                .map_err(|e| format!("Symbol spark_max_forward_decode not found: {}", e))?;
            let sample_token: Symbol<FnSampleToken> = lib
                .get(b"spark_max_sample_token\0")
                .map_err(|e| format!("Symbol spark_max_sample_token not found: {}", e))?;
            let kv_pool_register: Symbol<FnKvPoolRegister> = lib
                .get(b"spark_max_kv_pool_register\0")
                .map_err(|e| format!("Symbol spark_max_kv_pool_register not found: {}", e))?;
            let step_execute: Symbol<FnStepExecute> = lib
                .get(b"spark_max_step_execute\0")
                .map_err(|e| format!("Symbol spark_max_step_execute not found: {}", e))?;

            Ok(Self {
                version: *version,
                session_create: *session_create,
                session_destroy: *session_destroy,
                compute_scalar: *compute_scalar,
                load_model: *load_model,
                forward_prefill: *forward_prefill,
                forward_decode: *forward_decode,
                sample_token: *sample_token,
                kv_pool_register: *kv_pool_register,
                step_execute: *step_execute,
                _lib: lib,
            })
        }
    }

    pub fn global() -> Result<&'static Self, String> {
        let res = BINDINGS.get_or_init(|| {
            let path = default_so_path();
            Self::load_from(path)
        });

        match res {
            Ok(ref b) => Ok(b),
            Err(ref e) => Err(e.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_and_execute_mojo_c_abi() {
        let path = default_so_path();
        if !path.exists() {
            eprintln!("Notice: libspark_max.so not found at {:?}, skipping", path);
            return;
        }

        let bindings = match SparkMaxBindings::load_from(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("Notice: dlopen failed for {:?}: {}, skipping", path, e);
                return;
            }
        };

        let ver = unsafe { (bindings.version)() };
        assert_eq!(ver, 1);

        let session_id = unsafe { (bindings.session_create)(0) };
        assert!(session_id > 1000);

        let res = unsafe { (bindings.compute_scalar)(session_id, 10.0) };
        let expected = 10.0 * 2.5 + session_id as f32;
        assert_eq!(res, expected);

        let model_res = unsafe { (bindings.load_model)(session_id, 28, 3584, 28, 4, 128, 152064) };
        assert_eq!(model_res, 0);

        let pool_res = unsafe { (bindings.kv_pool_register)(session_id, 0x10000000, 1024 * 1024, 64, 16) };
        assert_eq!(pool_res, 0);

        let prefill_ms = unsafe { (bindings.forward_prefill)(session_id, 512, 1, 32) };
        assert!(prefill_ms >= 0.0);

        let decode_ms = unsafe { (bindings.forward_decode)(session_id, 1, 32) };
        assert!(decode_ms >= 0.0);

        let step_ms = unsafe { (bindings.step_execute)(session_id, 1, 512, 0, 0, 0, 0, 0, 0) };
        assert!(step_ms >= 0.0);

        let tok = unsafe { (bindings.sample_token)(session_id, 100, 1) };
        assert!(tok >= 100);

        let destroyed = unsafe { (bindings.session_destroy)(session_id) };
        assert_eq!(destroyed, 0);
    }
}
