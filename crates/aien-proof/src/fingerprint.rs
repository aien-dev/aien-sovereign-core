//! Job fingerprints. Paths are hashed relative to the job's base directory, so
//! the same code in two different worktrees produces the same key and shares
//! one stamp.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Directory names never hashed: build output, git internals, live crumb scents.
const SKIP: &[&str] = &["target", ".git", ".crumb.local"];

fn field(h: &mut blake3::Hasher, bytes: &[u8]) {
    h.update(&(bytes.len() as u64).to_be_bytes());
    h.update(bytes);
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let meta = fs::metadata(path)?;
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            if SKIP.iter().any(|s| name == *s) {
                continue;
            }
            collect(&entry.path(), out)?;
        }
    } else if meta.is_file() {
        out.push(path.to_path_buf());
    }
    Ok(())
}

pub fn job_key(
    base: &Path,
    job: &str,
    cmd: &[String],
    toolchain: &str,
    inputs: &[PathBuf],
) -> io::Result<String> {
    let mut h = blake3::Hasher::new();
    h.update(b"AIEN_PROOF_JOB_V1");
    field(&mut h, job.as_bytes());
    h.update(&(cmd.len() as u64).to_be_bytes());
    for arg in cmd {
        field(&mut h, arg.as_bytes());
    }
    field(&mut h, toolchain.as_bytes());

    let mut files = Vec::new();
    for input in inputs {
        collect(&base.join(input), &mut files)?;
    }
    files.sort();
    files.dedup();
    h.update(&(files.len() as u64).to_be_bytes());
    for file in &files {
        let rel = file.strip_prefix(base).unwrap_or(file);
        field(&mut h, rel.to_string_lossy().as_bytes());
        field(&mut h, blake3::hash(&fs::read(file)?).as_bytes());
    }
    Ok(h.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    fn tree(root: &Path) {
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("src/target")).unwrap();
        fs::write(root.join("src/lib.rs"), "pub fn a() {}").unwrap();
        fs::write(root.join("src/target/junk.o"), "junk").unwrap();
    }

    fn key(root: &Path) -> String {
        job_key(root, "t", &["cargo".into(), "test".into()], "rustc 1", &[PathBuf::from("src")]).unwrap()
    }

    #[test]
    fn same_code_in_two_worktrees_shares_a_key() {
        let (a, b) = (temp_dir("fp-a"), temp_dir("fp-b"));
        tree(&a);
        tree(&b);
        assert_eq!(key(&a), key(&b));
    }

    #[test]
    fn content_change_changes_key_and_build_output_does_not() {
        let root = temp_dir("fp-c");
        tree(&root);
        let before = key(&root);
        fs::write(root.join("src/target/junk.o"), "different junk").unwrap();
        assert_eq!(before, key(&root));
        fs::write(root.join("src/lib.rs"), "pub fn b() {}").unwrap();
        assert_ne!(before, key(&root));
    }

    #[test]
    fn command_and_toolchain_are_part_of_the_key() {
        let root = temp_dir("fp-d");
        tree(&root);
        let inputs = [PathBuf::from("src")];
        let base = job_key(&root, "t", &["x".into()], "rustc 1", &inputs).unwrap();
        assert_ne!(base, job_key(&root, "t", &["y".into()], "rustc 1", &inputs).unwrap());
        assert_ne!(base, job_key(&root, "t", &["x".into()], "rustc 2", &inputs).unwrap());
    }
}
