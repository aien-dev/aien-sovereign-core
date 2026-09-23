//! Exclusive advisory file locks (flock). The kernel releases a lock when its
//! holder exits or dies, so a crashed run can never leave a stale key behind.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::unix::io::AsRawFd;
use std::path::Path;

pub struct FileLock {
    _file: File,
}

fn open(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
}

impl FileLock {
    /// Block until the lock is ours.
    pub fn acquire(path: &Path) -> io::Result<Self> {
        let file = open(path)?;
        loop {
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                return Ok(Self { _file: file });
            }
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::Interrupted {
                return Err(err);
            }
        }
    }

    /// Take the lock if it is free right now; `None` if someone else holds it.
    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        let file = open(path)?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Some(Self { _file: file }));
        }
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EWOULDBLOCK) {
            Ok(None)
        } else {
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    #[test]
    fn second_holder_is_refused_until_release() {
        let path = temp_dir("lock").join("k.lock");
        let first = FileLock::try_acquire(&path).unwrap();
        assert!(first.is_some());
        assert!(FileLock::try_acquire(&path).unwrap().is_none());
        drop(first);
        assert!(FileLock::try_acquire(&path).unwrap().is_some());
    }
}
