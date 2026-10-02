//! Spawn a command without a shell, capture stdout and stderr, enforce a
//! wall-clock timeout by killing the whole process group (SIGTERM, then
//! SIGKILL after a grace period). No async: one waiter thread and two reader
//! threads per child.

use std::io::{self, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

pub struct RunSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    /// Time between SIGTERM and SIGKILL (ADR 0028 Decision 6 says 10 s).
    pub kill_grace: Duration,
}

#[derive(Debug)]
pub struct RunOutput {
    /// Exit code; None if the child was killed by a signal.
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub duration_ms: u64,
}

type Waited = io::Result<ExitStatus>;

fn signal_group(pid: i32, sig: i32) {
    if pid > 0 {
        // SAFETY: plain syscall; a stale group id yields ESRCH, ignored.
        unsafe {
            libc::kill(-pid, sig);
        }
    }
}

fn lost() -> io::Error {
    io::Error::other("waiter thread lost")
}

fn spawn(spec: &RunSpec) -> io::Result<Child> {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut attempt = 0;
    loop {
        match cmd.spawn() {
            Ok(c) => return Ok(c),
            // A script written moments ago can be briefly busy when another
            // thread forks while its write descriptor is still open.
            Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) && attempt < 40 => {
                attempt += 1;
                thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}

fn terminate(pid: i32, rx: &mpsc::Receiver<Waited>, grace: Duration) -> io::Result<Waited> {
    signal_group(pid, libc::SIGTERM);
    match rx.recv_timeout(grace) {
        Ok(r) => Ok(r),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            signal_group(pid, libc::SIGKILL);
            rx.recv().map_err(|_| lost())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(lost()),
    }
}

fn reader<R: Read + Send + 'static>(src: Option<R>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut s) = src {
            let _ = s.read_to_end(&mut buf);
        }
        buf
    })
}

pub fn run(spec: &RunSpec) -> io::Result<RunOutput> {
    let start = Instant::now();
    let mut child = spawn(spec)?;
    let pid = child.id() as i32;
    let h_out = reader(child.stdout.take());
    let h_err = reader(child.stderr.take());
    let (tx, rx) = mpsc::channel::<Waited>();
    thread::spawn(move || {
        let r = child.wait();
        let _ = tx.send(r);
    });

    let mut timed_out = false;
    let waited: Waited = match rx.recv_timeout(spec.timeout) {
        Ok(r) => r,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            timed_out = true;
            terminate(pid, &rx, spec.kill_grace)?
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => return Err(lost()),
    };
    // Whatever the leader did, nothing it started may outlive the run.
    signal_group(pid, libc::SIGKILL);
    let status = waited?;
    let stdout = h_out.join().unwrap_or_default();
    let stderr = h_err.join().unwrap_or_default();
    Ok(RunOutput {
        exit_code: status.code(),
        signal: status.signal(),
        stdout,
        stderr,
        timed_out,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str, timeout_ms: u64) -> RunSpec {
        RunSpec {
            program: PathBuf::from("/bin/sh"),
            args: vec!["-c".to_string(), script.to_string()],
            cwd: PathBuf::from("/"),
            timeout: Duration::from_millis(timeout_ms),
            kill_grace: Duration::from_millis(500),
        }
    }

    #[test]
    fn captures_output_and_exit_code() {
        let o = run(&sh("echo out; echo err 1>&2; exit 3", 10_000)).unwrap();
        assert_eq!(o.exit_code, Some(3));
        assert_eq!(o.stdout, b"out\n");
        assert_eq!(o.stderr, b"err\n");
        assert!(!o.timed_out);
    }

    #[test]
    fn timeout_kills_and_flags() {
        let o = run(&sh("sleep 20", 300)).unwrap();
        assert!(o.timed_out);
        assert_eq!(o.exit_code, None);
        assert!(o.signal.is_some());
        assert!(o.duration_ms < 10_000, "took {} ms", o.duration_ms);
    }

    #[test]
    fn timeout_kills_the_whole_group() {
        // The grandchild inherits stdout; if only the leader were killed the
        // reader threads would block until the grandchild's sleep ends.
        let o = run(&sh("sleep 20 & wait", 300)).unwrap();
        assert!(o.timed_out);
        assert!(o.duration_ms < 10_000, "took {} ms", o.duration_ms);
    }

    #[test]
    fn term_ignored_gets_killed_after_grace() {
        let o = run(&sh("trap '' TERM; sleep 20", 200)).unwrap();
        assert!(o.timed_out);
        assert!(o.duration_ms < 10_000, "took {} ms", o.duration_ms);
    }

    #[test]
    fn orphans_do_not_outlive_a_normal_exit() {
        let o = run(&sh("sleep 20 & echo done", 10_000)).unwrap();
        assert_eq!(o.exit_code, Some(0));
        assert!(!o.timed_out);
        assert!(o.duration_ms < 10_000, "took {} ms", o.duration_ms);
    }

    #[test]
    fn missing_program_is_an_error() {
        let mut s = sh("true", 1000);
        s.program = PathBuf::from("/nonexistent/aien-test-nope");
        assert!(run(&s).is_err());
    }
}
