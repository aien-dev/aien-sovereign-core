//! EvidenceReceiptV1 (ADR 0028 Decision 4): canonical JSON, sha256 digest,
//! content-addressed stdout/stderr blobs, immutable write (never overwrite).

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::{ErrorKind, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = "aien-test/EvidenceReceiptV1";

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    let digest = h.finalize();
    let mut s = String::with_capacity(64);
    for b in digest.iter() {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// A float or other value that canonical encoding forbids (C3).
    Canon(String),
    Io(String),
    /// A file already exists at the content address with different bytes.
    Collision(String),
    /// Bytes read back after writing did not match their digest.
    Verify(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Canon(s) => write!(f, "canonical encoding error: {s}"),
            StoreError::Io(s) => write!(f, "io error: {s}"),
            StoreError::Collision(s) => write!(f, "evidence collision: {s}"),
            StoreError::Verify(s) => write!(f, "evidence verify failed: {s}"),
        }
    }
}

fn io_err(what: &str, e: std::io::Error) -> StoreError {
    StoreError::Io(format!("{what}: {e}"))
}

fn write_str(s: &str, out: &mut Vec<u8>) {
    out.push(b'"');
    for c in s.chars() {
        match c {
            '"' => out.extend_from_slice(b"\\\""),
            '\\' => out.extend_from_slice(b"\\\\"),
            '\n' => out.extend_from_slice(b"\\n"),
            '\r' => out.extend_from_slice(b"\\r"),
            '\t' => out.extend_from_slice(b"\\t"),
            c if (c as u32) < 0x20 => {
                out.extend_from_slice(format!("\\u{:04x}", c as u32).as_bytes())
            }
            c => {
                let mut buf = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
    }
    out.push(b'"');
}

fn write_value(v: &Value, out: &mut Vec<u8>) -> Result<(), StoreError> {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.extend_from_slice(i.to_string().as_bytes());
            } else if let Some(u) = n.as_u64() {
                out.extend_from_slice(u.to_string().as_bytes());
            } else {
                return Err(StoreError::Canon(format!(
                    "floating point not allowed: {n}"
                )));
            }
        }
        Value::String(s) => write_str(s, out),
        Value::Array(a) => {
            out.push(b'[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_value(x, out)?;
            }
            out.push(b']');
        }
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            out.push(b'{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_str(k, out);
                out.push(b':');
                write_value(&m[k.as_str()], out)?;
            }
            out.push(b'}');
        }
    }
    Ok(())
}

/// Canonical bytes (C1 to C5): sorted keys, integers only, minimal escapes.
pub fn canonical(v: &Value) -> Result<Vec<u8>, StoreError> {
    let mut out = Vec::new();
    write_value(v, &mut out)?;
    Ok(out)
}

pub fn canonical_digest(v: &Value) -> Result<String, StoreError> {
    Ok(sha256_hex(&canonical(v)?))
}

pub fn build_receipt(core: Value, volatile: Value) -> Value {
    json!({ "schema": SCHEMA, "core": core, "volatile": volatile })
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: &Path) -> Store {
        Store {
            dir: dir.to_path_buf(),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Store a blob at `blobs/<sha256>.log` via temp file and rename, then
    /// re-verify. An existing blob with the same digest is success; an
    /// existing file with other bytes is a collision.
    pub fn put_blob(&self, bytes: &[u8]) -> Result<String, StoreError> {
        let sha = sha256_hex(bytes);
        let blobs = self.dir.join("blobs");
        fs::create_dir_all(&blobs).map_err(|e| io_err("create blobs dir", e))?;
        let path = blobs.join(format!("{sha}.log"));
        if path.exists() {
            let existing = fs::read(&path).map_err(|e| io_err("read blob", e))?;
            if sha256_hex(&existing) != sha {
                return Err(StoreError::Collision(format!(
                    "blob {sha} exists with other bytes"
                )));
            }
            return Ok(sha);
        }
        let tmp = blobs.join(format!(".tmp-{sha}-{}", std::process::id()));
        {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o444)
                .open(&tmp)
                .map_err(|e| io_err("create blob temp", e))?;
            f.write_all(bytes).map_err(|e| io_err("write blob", e))?;
            f.sync_all().map_err(|e| io_err("sync blob", e))?;
        }
        fs::rename(&tmp, &path).map_err(|e| io_err("rename blob", e))?;
        let back = fs::read(&path).map_err(|e| io_err("read back blob", e))?;
        if sha256_hex(&back) != sha {
            return Err(StoreError::Verify(format!(
                "blob {sha} digest mismatch after write"
            )));
        }
        Ok(sha)
    }

    /// Write `<digest>.json` exclusively, mode 0444, canonical bytes plus one
    /// LF. Identical existing content is success; different content is a
    /// fatal collision. Never overwrites.
    pub fn put_receipt(&self, receipt: &Value) -> Result<(String, PathBuf), StoreError> {
        let bytes = canonical(receipt)?;
        let digest = sha256_hex(&bytes);
        fs::create_dir_all(&self.dir).map_err(|e| io_err("create evidence dir", e))?;
        let path = self.dir.join(format!("{digest}.json"));
        let mut want = bytes;
        want.push(b'\n');
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true).mode(0o444);
        match opts.open(&path) {
            Ok(mut f) => {
                f.write_all(&want).map_err(|e| io_err("write receipt", e))?;
                f.sync_all().map_err(|e| io_err("sync receipt", e))?;
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                let existing = fs::read(&path).map_err(|e| io_err("read existing receipt", e))?;
                if existing != want {
                    return Err(StoreError::Collision(format!(
                        "receipt {digest} exists with different bytes"
                    )));
                }
            }
            Err(e) => return Err(io_err("open receipt", e)),
        }
        let back = fs::read(&path).map_err(|e| io_err("read back receipt", e))?;
        if back != want {
            return Err(StoreError::Verify(format!(
                "receipt {digest} differs after write"
            )));
        }
        Ok((digest, path))
    }
}

/// Format seconds since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
pub fn utc_string(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days algorithm.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn canonical_sorts_keys_and_is_compact() {
        let v = json!({"b": [1, true, null], "a": {"z": "x", "y": -3}});
        assert_eq!(
            String::from_utf8(canonical(&v).unwrap()).unwrap(),
            r#"{"a":{"y":-3,"z":"x"},"b":[1,true,null]}"#
        );
    }

    #[test]
    fn canonical_escapes_per_c4() {
        let v = json!("a\"b\\c\nd\r\te\u{1f}f\u{7f}\u{e9}");
        let s = String::from_utf8(canonical(&v).unwrap()).unwrap();
        assert_eq!(s, "\"a\\\"b\\\\c\\nd\\r\\te\\u001ff\u{7f}\u{e9}\"");
    }

    #[test]
    fn canonical_rejects_floats() {
        assert!(canonical(&json!({"x": 1.5})).is_err());
        assert!(canonical(&json!([0.0])).is_err());
    }

    #[test]
    fn canonical_digest_is_stable_across_key_order() {
        let a = json!({"x": 1, "y": 2});
        let b = json!({"y": 2, "x": 1});
        assert_eq!(canonical_digest(&a).unwrap(), canonical_digest(&b).unwrap());
    }

    #[test]
    fn utc_formatting() {
        assert_eq!(utc_string(0), "1970-01-01T00:00:00Z");
        assert_eq!(utc_string(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(utc_string(951_782_400), "2000-02-29T00:00:00Z");
    }

    #[test]
    fn receipt_roundtrip_mode_and_name() {
        let t = TempDir::new("ev1");
        let store = Store::new(t.path());
        let r = build_receipt(json!({"gate": "G"}), json!({"duration_ms": 5}));
        let (digest, path) = store.put_receipt(&r).unwrap();
        assert_eq!(path, t.path().join(format!("{digest}.json")));
        let bytes = fs::read(&path).unwrap();
        assert_eq!(*bytes.last().unwrap(), b'\n');
        assert_eq!(sha256_hex(&bytes[..bytes.len() - 1]), digest);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o444);
        let back: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back["schema"], SCHEMA);
    }

    #[test]
    fn identical_receipt_twice_is_success() {
        let t = TempDir::new("ev2");
        let store = Store::new(t.path());
        let r = build_receipt(json!({"gate": "G"}), json!({}));
        let a = store.put_receipt(&r).unwrap();
        let b = store.put_receipt(&r).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn receipt_overwrite_is_refused() {
        let t = TempDir::new("ev3");
        let store = Store::new(t.path());
        let r = build_receipt(json!({"gate": "G"}), json!({}));
        let (_, path) = store.put_receipt(&r).unwrap();
        // Tamper with the stored file, then try to write the same receipt again.
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        fs::write(&path, b"tampered").unwrap();
        match store.put_receipt(&r) {
            Err(StoreError::Collision(_)) => {}
            other => panic!("expected collision, got {other:?}"),
        }
        assert_eq!(fs::read(&path).unwrap(), b"tampered");
    }

    #[test]
    fn blob_is_content_addressed_and_idempotent() {
        let t = TempDir::new("ev4");
        let store = Store::new(t.path());
        let sha = store.put_blob(b"hello\n").unwrap();
        assert_eq!(sha, sha256_hex(b"hello\n"));
        let p = t.path().join("blobs").join(format!("{sha}.log"));
        assert_eq!(fs::read(&p).unwrap(), b"hello\n");
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o444
        );
        assert_eq!(store.put_blob(b"hello\n").unwrap(), sha);
    }

    #[test]
    fn blob_collision_detected() {
        let t = TempDir::new("ev5");
        let store = Store::new(t.path());
        let sha = store.put_blob(b"hello\n").unwrap();
        let p = t.path().join("blobs").join(format!("{sha}.log"));
        fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
        fs::write(&p, b"evil").unwrap();
        match store.put_blob(b"hello\n") {
            Err(StoreError::Collision(_)) => {}
            other => panic!("expected collision, got {other:?}"),
        }
    }
}
