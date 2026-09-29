// spark-aegis Mojo SIMD Dynamic Bridge
// Pure compiled C-ABI dynamic bridge with pure Rust fallback

use libloading::{Library, Symbol};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

type FnVersion = unsafe extern "C" fn() -> i32;
type FnScanByte = unsafe extern "C" fn(*const u8, usize, u8) -> isize;
type FnCountMatches = unsafe extern "C" fn(*const u8, usize, u8) -> isize;
type FnFindPattern = unsafe extern "C" fn(*const u8, usize, *const u8, usize) -> isize;

pub struct MojoBindings {
    _lib: Library,
    pub version: FnVersion,
    pub scan_byte: FnScanByte,
    pub count_matches: FnCountMatches,
    pub find_pattern: FnFindPattern,
}

static BINDINGS: OnceLock<Option<MojoBindings>> = OnceLock::new();

pub fn default_so_path() -> PathBuf {
    if let Ok(p) = std::env::var("SPARK_AEGIS_SO_PATH") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return pb;
        }
    }
    if let Some(p) = option_env!("SPARK_AEGIS_SO_BUILT") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return pb;
        }
    }
    let local = PathBuf::from("mojo/libsimd_matcher.so");
    if local.exists() {
        return local;
    }
    let local_crates = PathBuf::from("crates/spark-aegis/mojo/libsimd_matcher.so");
    if local_crates.exists() {
        return local_crates;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let candidate = PathBuf::from(home).join("workspace/spark-aegis/mojo/libsimd_matcher.so");
    if candidate.exists() {
        return candidate;
    }
    PathBuf::from("mojo/libsimd_matcher.so")
}

impl MojoBindings {
    pub fn load_from<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let p = path.as_ref();
        if !p.exists() {
            return Err(format!("Shared library not found at {:?}", p));
        }
        unsafe {
            let lib = Library::new(p).map_err(|e| format!("Failed to dlopen {:?}: {}", p, e))?;
            let version: Symbol<FnVersion> = lib
                .get(b"mojo_simd_version\0")
                .map_err(|e| format!("Symbol mojo_simd_version not found: {}", e))?;
            let scan_byte: Symbol<FnScanByte> = lib
                .get(b"mojo_simd_scan_byte\0")
                .map_err(|e| format!("Symbol mojo_simd_scan_byte not found: {}", e))?;
            let count_matches: Symbol<FnCountMatches> = lib
                .get(b"mojo_simd_count_matches\0")
                .map_err(|e| format!("Symbol mojo_simd_count_matches not found: {}", e))?;
            let find_pattern: Symbol<FnFindPattern> = lib
                .get(b"mojo_simd_find_pattern\0")
                .map_err(|e| format!("Symbol mojo_simd_find_pattern not found: {}", e))?;

            Ok(Self {
                version: *version,
                scan_byte: *scan_byte,
                count_matches: *count_matches,
                find_pattern: *find_pattern,
                _lib: lib,
            })
        }
    }

    pub fn global() -> Option<&'static Self> {
        BINDINGS
            .get_or_init(|| {
                let path = default_so_path();
                MojoBindings::load_from(path).ok()
            })
            .as_ref()
    }
}

pub fn scan_byte(data: &[u8], target: u8) -> Option<usize> {
    if let Some(bindings) = MojoBindings::global() {
        let idx = unsafe { (bindings.scan_byte)(data.as_ptr(), data.len(), target) };
        if idx >= 0 {
            return Some(idx as usize);
        }
        return None;
    }
    // Pure Rust fallback
    data.iter().position(|&b| b == target)
}

pub fn count_byte_matches(data: &[u8], target: u8) -> usize {
    if let Some(bindings) = MojoBindings::global() {
        let count = unsafe { (bindings.count_matches)(data.as_ptr(), data.len(), target) };
        if count >= 0 {
            return count as usize;
        }
    }
    // Pure Rust fallback
    data.iter().filter(|&&b| b == target).count()
}

pub fn find_pattern(data: &[u8], pattern: &[u8]) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    if data.len() < pattern.len() {
        return None;
    }
    if let Some(bindings) = MojoBindings::global() {
        let idx = unsafe {
            (bindings.find_pattern)(data.as_ptr(), data.len(), pattern.as_ptr(), pattern.len())
        };
        if idx >= 0 {
            return Some(idx as usize);
        }
        return None;
    }
    // Pure Rust fallback
    data.windows(pattern.len())
        .position(|window| window == pattern)
}

pub fn is_simd_accelerated() -> bool {
    MojoBindings::global().is_some()
}

pub fn engine_name() -> &'static str {
    if is_simd_accelerated() {
        "Mojo SIMD Hardware Acceleration"
    } else {
        "Pure Rust Compiled Fallback"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_byte_scan_and_pattern_search() {
        let buffer = b"SOVEREIGN_SYSTEMS_SPARK_AEGIS_SCAN";
        let target = b'P';
        let found = scan_byte(buffer, target);
        assert_eq!(found, Some(19));

        let not_found = scan_byte(buffer, b'Z');
        assert_eq!(not_found, None);

        let pattern = b"SPARK_AEGIS";
        let pat_idx = find_pattern(buffer, pattern);
        assert_eq!(pat_idx, Some(18));

        let missing_pat = b"NONEXISTENT";
        assert_eq!(find_pattern(buffer, missing_pat), None);

        let count_s = count_byte_matches(buffer, b'S');
        assert_eq!(count_s, 7);
    }

    #[test]
    fn test_engine_reporting() {
        let name = engine_name();
        assert!(!name.is_empty());
    }
}
