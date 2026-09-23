//! The board: look up the stamp, join an identical run in flight, or claim the
//! job and run it behind the CPU slot, memory, and GPU gates.
//!
//! Board layout (default `~/.local/state/aien-proof`, override `AIEN_PROOF_DIR`):
//!   stamps/<key>.json   passing runs, reused by every agent and worktree
//!   fails/<key>.json    latest failing run, shared only with runs that joined it
//!   locks/<key>.lock    held while a job runs; identical requests block on it
//!   slots/cpu-N.lock    CPU slots (`AIEN_PROOF_SLOTS`)
//!   slots/gpu.lock      the GPU key, one GPU job at a time
//!   logs/<key>.log      full output of the latest real run
//!   ledger.jsonl        hash-chained record of every real run

use crate::{fingerprint, ledger, lock::FileLock};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const GIB: u64 = 1024 * 1024 * 1024;

pub struct Board {
    pub root: PathBuf,
    pub cpu_slots: usize,
    pub min_free_bytes: u64,
    /// Longest wait for free memory before running anyway (a gate that never
    /// opens would hang the hive).
    pub mem_wait: Duration,
    pub quiet: bool,
}

pub struct Job {
    pub name: String,
    pub agent: String,
    /// Directory the command runs in; inputs are relative to it.
    pub base: PathBuf,
    pub inputs: Vec<PathBuf>,
    pub cmd: Vec<String>,
    pub gpu: bool,
    pub toolchain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub key: String,
    pub job: String,
    pub agent: String,
    pub cmd: Vec<String>,
    pub exit_code: i32,
    pub duration_ms: u64,
    pub finished_at: u64,
    pub ledger_index: u64,
    pub ledger_hash: String,
}

#[derive(Debug)]
pub enum Outcome {
    /// A passing stamp already existed; nothing ran.
    Stamped(Record),
    /// An identical run was in flight; its result was shared, nothing ran.
    Joined(Record),
    /// This request ran the job.
    Ran(Record),
}

impl Outcome {
    pub fn record(&self) -> &Record {
        match self {
            Outcome::Stamped(r) | Outcome::Joined(r) | Outcome::Ran(r) => r,
        }
    }

    pub fn passed(&self) -> bool {
        self.record().exit_code == 0
    }

    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Stamped(_) => "stamped",
            Outcome::Joined(_) => "joined",
            Outcome::Ran(_) => "ran",
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn env_num(name: &str) -> Option<u64> {
    std::env::var(name).ok()?.trim().parse().ok()
}

/// `MemAvailable` from /proc/meminfo, in bytes.
pub fn mem_available() -> Option<u64> {
    let info = fs::read_to_string("/proc/meminfo").ok()?;
    let line = info.lines().find(|l| l.starts_with("MemAvailable:"))?;
    let kb: u64 = line.trim_start_matches("MemAvailable:").trim().trim_end_matches("kB").trim().parse().ok()?;
    Some(kb * 1024)
}

fn tee<R: Read + Send + 'static>(
    mut src: R,
    log: Arc<Mutex<fs::File>>,
    echo: bool,
    to_stderr: bool,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        loop {
            let n = match src.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if let Ok(mut f) = log.lock() {
                let _ = f.write_all(&buf[..n]);
            }
            if echo {
                if to_stderr {
                    let _ = io::stderr().write_all(&buf[..n]);
                } else {
                    let _ = io::stdout().write_all(&buf[..n]);
                }
            }
        }
    })
}

impl Board {
    pub fn from_env() -> Self {
        let root = std::env::var_os("AIEN_PROOF_DIR").map(PathBuf::from).unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
            home.join(".local/state/aien-proof")
        });
        let cores = thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        Self {
            root,
            cpu_slots: env_num("AIEN_PROOF_SLOTS").map(|n| n as usize).unwrap_or((cores / 8).max(2)),
            min_free_bytes: env_num("AIEN_PROOF_MIN_FREE_GB").unwrap_or(4) * GIB,
            mem_wait: Duration::from_secs(env_num("AIEN_PROOF_MEM_WAIT_SECS").unwrap_or(600)),
            quiet: false,
        }
    }

    fn note(&self, msg: &str) {
        if !self.quiet {
            eprintln!("[aien-proof] {msg}");
        }
    }

    fn record_path(&self, kind: &str, key: &str) -> PathBuf {
        self.root.join(kind).join(format!("{key}.json"))
    }

    fn read_record(&self, kind: &str, key: &str) -> Option<Record> {
        serde_json::from_slice(&fs::read(self.record_path(kind, key)).ok()?).ok()
    }

    fn write_record(&self, kind: &str, rec: &Record) -> io::Result<()> {
        let path = self.record_path(kind, &rec.key);
        fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        fs::write(&tmp, serde_json::to_vec_pretty(rec)?)?;
        fs::rename(tmp, path)
    }

    fn replay(&self, how: &str, rec: &Record) {
        if self.quiet {
            return;
        }
        let verdict = if rec.exit_code == 0 { "pass" } else { "FAIL" };
        eprintln!(
            "[aien-proof] {}: {how} {verdict} (ran by {} in {} ms, ledger #{}), replaying output",
            rec.job, rec.agent, rec.duration_ms, rec.ledger_index
        );
        if let Ok(log) = fs::read(self.root.join("logs").join(format!("{}.log", rec.key))) {
            let _ = io::stdout().write_all(&log);
        }
    }

    fn acquire_cpu_slot(&self) -> io::Result<FileLock> {
        let mut noted = false;
        loop {
            for i in 0..self.cpu_slots.max(1) {
                if let Some(slot) = FileLock::try_acquire(&self.root.join("slots").join(format!("cpu-{i}.lock")))? {
                    return Ok(slot);
                }
            }
            if !noted {
                self.note(&format!("all {} CPU slots busy, waiting", self.cpu_slots));
                noted = true;
            }
            thread::sleep(Duration::from_millis(100));
        }
    }

    fn wait_for_memory(&self) {
        let start = Instant::now();
        let mut noted = false;
        while let Some(free) = mem_available() {
            if free >= self.min_free_bytes {
                return;
            }
            if start.elapsed() >= self.mem_wait {
                self.note("memory still low after the wait limit, running anyway");
                return;
            }
            if !noted {
                self.note(&format!(
                    "{} GiB free, below the {} GiB gate, waiting",
                    free / GIB,
                    self.min_free_bytes / GIB
                ));
                noted = true;
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    pub fn run(&self, job: &Job) -> io::Result<Outcome> {
        if job.cmd.is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty command"));
        }
        let key = fingerprint::job_key(&job.base, &job.name, &job.cmd, &job.toolchain, &job.inputs)?;
        if let Some(rec) = self.read_record("stamps", &key) {
            self.replay("stamped", &rec);
            return Ok(Outcome::Stamped(rec));
        }

        let lock_path = self.root.join("locks").join(format!("{key}.lock"));
        let _job_lock = match FileLock::try_acquire(&lock_path)? {
            Some(lock) => lock,
            None => {
                let waited_from = now();
                self.note(&format!("{}: identical run already in flight, joining it", job.name));
                let lock = FileLock::acquire(&lock_path)?;
                if let Some(rec) = self.read_record("stamps", &key) {
                    self.replay("joined", &rec);
                    return Ok(Outcome::Joined(rec));
                }
                if let Some(rec) = self.read_record("fails", &key) {
                    if rec.finished_at >= waited_from {
                        self.replay("joined", &rec);
                        return Ok(Outcome::Joined(rec));
                    }
                }
                lock
            }
        };
        // A run may have finished between the first lookup and taking the lock.
        if let Some(rec) = self.read_record("stamps", &key) {
            self.replay("stamped", &rec);
            return Ok(Outcome::Stamped(rec));
        }

        let _cpu = self.acquire_cpu_slot()?;
        self.wait_for_memory();
        let _gpu = if job.gpu {
            let gpu_path = self.root.join("slots/gpu.lock");
            Some(match FileLock::try_acquire(&gpu_path)? {
                Some(lock) => lock,
                None => {
                    self.note("GPU key in use, waiting");
                    FileLock::acquire(&gpu_path)?
                }
            })
        } else {
            None
        };

        let log_path = self.root.join("logs").join(format!("{key}.log"));
        fs::create_dir_all(log_path.parent().unwrap())?;
        let log = Arc::new(Mutex::new(fs::File::create(&log_path)?));
        let started = Instant::now();
        let mut child = Command::new(&job.cmd[0])
            .args(&job.cmd[1..])
            .current_dir(&job.base)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let out = tee(child.stdout.take().unwrap(), log.clone(), !self.quiet, false);
        let err = tee(child.stderr.take().unwrap(), log.clone(), !self.quiet, true);
        let status = child.wait()?;
        let _ = out.join();
        let _ = err.join();
        let duration_ms = started.elapsed().as_millis() as u64;
        let exit_code = status.code().unwrap_or(-1);

        let payload = fs::read(&log_path)?;
        let verdict = if exit_code == 0 { "pass" } else { "fail" };
        let intent = format!("{verdict} exit={exit_code} key={key}");
        let event = ledger::append(&self.root, &job.agent, &job.name, &intent, &payload)?;
        let rec = Record {
            key,
            job: job.name.clone(),
            agent: job.agent.clone(),
            cmd: job.cmd.clone(),
            exit_code,
            duration_ms,
            finished_at: now(),
            ledger_index: event.index,
            ledger_hash: ledger::hex(&event.hash),
        };
        self.write_record(if exit_code == 0 { "stamps" } else { "fails" }, &rec)?;
        Ok(Outcome::Ran(rec))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    fn board(tag: &str) -> Board {
        Board {
            root: temp_dir(tag),
            cpu_slots: 4,
            min_free_bytes: 0,
            mem_wait: Duration::from_secs(1),
            quiet: true,
        }
    }

    /// A job whose command appends a line to `runs` (outside its inputs), so the
    /// number of lines is the number of real executions.
    fn job(base: &std::path::Path, name: &str, script: &str) -> Job {
        fs::create_dir_all(base.join("src")).unwrap();
        fs::write(base.join("src/lib.rs"), "pub fn a() {}").unwrap();
        Job {
            name: name.into(),
            agent: "tester".into(),
            base: base.to_path_buf(),
            inputs: vec![PathBuf::from("src")],
            cmd: vec!["sh".into(), "-c".into(), format!("echo x >> runs; {script}")],
            gpu: false,
            toolchain: "test".into(),
        }
    }

    fn runs(base: &std::path::Path) -> usize {
        fs::read_to_string(base.join("runs")).map(|s| s.lines().count()).unwrap_or(0)
    }

    #[test]
    fn pass_is_stamped_then_replayed_without_running() {
        let b = board("board-stamp");
        let base = temp_dir("base-stamp");
        let j = job(&base, "unit", "true");
        assert!(matches!(b.run(&j).unwrap(), Outcome::Ran(_)));
        let second = b.run(&j).unwrap();
        assert!(matches!(second, Outcome::Stamped(_)));
        assert!(second.passed());
        assert_eq!(runs(&base), 1);
        assert_eq!(ledger::verify(&b.root.join(ledger::LEDGER_FILE)).unwrap().0, 1);
    }

    #[test]
    fn a_second_worktree_with_the_same_code_reuses_the_stamp() {
        let b = board("board-worktree");
        let (wt1, wt2) = (temp_dir("wt1"), temp_dir("wt2"));
        assert!(matches!(b.run(&job(&wt1, "unit", "true")).unwrap(), Outcome::Ran(_)));
        assert!(matches!(b.run(&job(&wt2, "unit", "true")).unwrap(), Outcome::Stamped(_)));
        assert_eq!(runs(&wt1) + runs(&wt2), 1);
    }

    #[test]
    fn identical_concurrent_requests_run_once() {
        let b = Arc::new(board("board-join"));
        let base = temp_dir("base-join");
        let j = Arc::new(job(&base, "slow", "sleep 1"));
        let handles: Vec<_> = (0..5)
            .map(|_| {
                let (b, j) = (b.clone(), j.clone());
                thread::spawn(move || b.run(&j).unwrap())
            })
            .collect();
        let outcomes: Vec<Outcome> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        assert_eq!(runs(&base), 1);
        assert_eq!(outcomes.iter().filter(|o| matches!(o, Outcome::Ran(_))).count(), 1);
        assert!(outcomes.iter().all(Outcome::passed));
    }

    #[test]
    fn failures_are_not_stamped_but_are_shared_with_joiners() {
        let b = Arc::new(board("board-fail"));
        let base = temp_dir("base-fail");
        let j = Arc::new(job(&base, "broken", "sleep 1; exit 3"));
        let handles: Vec<_> = (0..3)
            .map(|_| {
                let (b, j) = (b.clone(), j.clone());
                thread::spawn(move || b.run(&j).unwrap())
            })
            .collect();
        for h in handles {
            assert_eq!(h.join().unwrap().record().exit_code, 3);
        }
        assert_eq!(runs(&base), 1);
        // A fresh request after the failure runs again rather than trusting it.
        assert!(matches!(b.run(&j).unwrap(), Outcome::Ran(_)));
        assert_eq!(runs(&base), 2);
    }

    #[test]
    fn gpu_jobs_never_overlap() {
        let b = Arc::new(board("board-gpu"));
        let base = temp_dir("base-gpu");
        let started = Instant::now();
        let handles: Vec<_> = (0..2)
            .map(|i| {
                let b = b.clone();
                let mut j = job(&base, &format!("gpu-{i}"), "sleep 0.5");
                j.gpu = true;
                thread::spawn(move || b.run(&j).unwrap())
            })
            .collect();
        for h in handles {
            assert!(h.join().unwrap().passed());
        }
        assert!(started.elapsed() >= Duration::from_millis(1000));
    }
}
