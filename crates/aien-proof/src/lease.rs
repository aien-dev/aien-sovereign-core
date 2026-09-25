//! Exclusive resource leases (`aien-proof hold`) and receipt binding.
//!
//! A physical hardware receipt must prove the run held an exclusive lease on
//! the machine. `hold` takes one flock per `--resource` name, runs the held
//! command, captures its output, seals an audit event into the Crumb ledger,
//! and writes a hold record. Receipts reference the hold by ID; verification
//! checks the record, the ledger event, and the output digest. This reuses
//! the existing lock and ledger machinery. No second locking system.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::evidence::{Receipt, Verdict};
use crate::ledger;
use crate::lock::FileLock;

/// Subdirectory of the proof board root holding hold records and locks.
pub const HOLD_DIR: &str = "holds";
/// Subdirectory holding captured output of held runs.
pub const HOLD_LOG_DIR: &str = "hold-logs";
/// Ledger action used for hold audit events.
pub const HOLD_ACTION: &str = "hold";

/// A recorded exclusive hold on one or more resources.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldRecord {
    /// Hex of the hold audit event hash. This is the hold ID.
    pub hold_id: String,
    pub resources: Vec<String>,
    pub holder: String,
    pub command: Vec<String>,
    pub command_line: String,
    pub started: u64,
    pub ended: u64,
    pub exit_status: i32,
    /// BLAKE3 hex of the complete captured output.
    pub output_digest: String,
    pub ledger_index: u64,
    pub ledger_hash: String,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Resource names are lowercase alphanumerics, dashes, and underscores.
pub fn check_resource_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err(format!("invalid resource name {name:?}"));
    }
    let ok = name
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    if !ok {
        return Err(format!(
            "invalid resource name {name:?}: use [a-z0-9_-]"
        ));
    }
    Ok(())
}

fn lock_path(root: &Path, resource: &str) -> PathBuf {
    root.join(HOLD_DIR).join(format!("{resource}.lock"))
}

/// Acquire the exclusive lock for each resource, blocking. Returned guards
/// must stay alive for the whole protected run.
pub fn acquire_resources(root: &Path, resources: &[String]) -> io::Result<Vec<FileLock>> {
    let mut sorted = resources.to_vec();
    sorted.sort();
    sorted.dedup();
    let mut guards = Vec::with_capacity(sorted.len());
    for name in &sorted {
        guards.push(FileLock::acquire(&lock_path(root, name))?);
    }
    Ok(guards)
}

/// Load a hold record by ID. Missing or altered records are errors.
pub fn load_hold(store: &Path, hold_id: &str) -> Result<HoldRecord, String> {
    let normalized = hold_id.to_ascii_lowercase();
    if normalized.len() != 64 || !normalized.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("hold id {hold_id:?} is not a 64 digit hash"));
    }
    let path = store.join(HOLD_DIR).join(format!("{normalized}.json"));
    let text = fs::read(&path).map_err(|_| format!("missing hold record {normalized}"))?;
    let record: HoldRecord =
        serde_json::from_slice(&text).map_err(|e| format!("hold record invalid: {e}"))?;
    if record.hold_id.to_ascii_lowercase() != normalized {
        return Err("hold record id does not match its file name".to_string());
    }
    Ok(record)
}

/// Check a receipt's lease reference against the hold store and ledger.
/// Returns `None` when the receipt claims no lease. `Ok(Some(report))` is
/// the binding verdict; `Err` means the store itself is unreadable.
pub fn check_lease_ref(
    receipt: &Receipt,
    store: &Path,
) -> Result<Option<crate::evidence::Report>, String> {
    use crate::evidence::Report;
    let lease = match &receipt.lease {
        Some(l) => l,
        None => return Ok(None),
    };
    let record = load_hold(store, &lease.hold_id).map_err(|e| {
        format!("lease binding broken: {e}")
    })?;
    let mut problems = Vec::new();
    if !record.resources.iter().any(|r| r == &lease.resource) {
        problems.push(format!(
            "hold {} never covered resource {:?}",
            lease.hold_id, lease.resource
        ));
    }
    if receipt.machine != lease.resource {
        problems.push(format!(
            "receipt machine {:?} does not match leased resource {:?}",
            receipt.machine, lease.resource
        ));
    }
    if receipt.procedure != record.command_line {
        problems.push(format!(
            "receipt procedure {:?} does not match held command {:?}",
            receipt.procedure, record.command_line
        ));
    }
    if !receipt
        .output_digest
        .eq_ignore_ascii_case(&record.output_digest)
    {
        problems.push("receipt output digest does not match the held run output".to_string());
    }
    if receipt.result == Verdict::Pass && record.exit_status != 0 {
        problems.push(format!(
            "receipt claims PASS but the held run exited with {}",
            record.exit_status
        ));
    }
    // The hold audit event must exist in the ledger with a matching hash.
    let event = ledger::read_event(&store.join(ledger::LEDGER_FILE), record.ledger_index)?
        .ok_or_else(|| {
            format!(
                "lease binding broken: ledger event {} is missing",
                record.ledger_index
            )
        })?;
    if ledger::hex(&event.hash) != record.ledger_hash.to_ascii_lowercase()
        || ledger::hex(&event.hash) != record.hold_id.to_ascii_lowercase()
    {
        problems.push("hold record does not match its ledger audit event".to_string());
    }
    if event.action != HOLD_ACTION {
        problems.push(format!(
            "ledger event {} is not a hold audit event",
            record.ledger_index
        ));
    }
    if ledger::hex(&event.payload_hash) != record.output_digest.to_ascii_lowercase() {
        problems.push("hold output digest does not match the ledger payload hash".to_string());
    }
    if problems.is_empty() {
        Ok(Some(Report::pass("lease binding checks out")))
    } else {
        Ok(Some(Report::fail(problems)))
    }
}

/// Require a lease for hardware tiers. Called by chain and gate evaluation
/// so a Machine 1 qualification without a lease never reads as PASS.
pub fn lease_status(receipt: &Receipt, store: &Path) -> Option<crate::evidence::Report> {
    use crate::evidence::Report;
    if !receipt.tier.is_hardware() {
        return None;
    }
    match &receipt.lease {
        None => Some(Report::closed(
            Verdict::Incomplete,
            vec!["Machine 1 qualification has no exclusive hardware lease".to_string()],
        )),
        Some(_) => match check_lease_ref(receipt, store) {
            Ok(Some(r)) => Some(r),
            Ok(None) => None,
            Err(e) => Some(Report::fail(vec![e])),
        },
    }
}

pub struct HoldOutcome {
    pub record: HoldRecord,
    pub exit_code: i32,
}

/// Run `cmd` while holding every resource exclusively, then seal the audit
/// event and write the hold record. Output is teed to the caller's stdout
/// and stderr and captured for the digest.
pub fn hold_and_run(
    root: &Path,
    resources: &[String],
    holder: &str,
    job: &str,
    cmd: &[String],
    echo: bool,
) -> io::Result<HoldOutcome> {
    for name in resources {
        check_resource_name(name)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    }
    if cmd.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "empty command"));
    }
    let mut sorted = resources.to_vec();
    sorted.sort();
    sorted.dedup();
    fs::create_dir_all(root.join(HOLD_DIR))?;
    fs::create_dir_all(root.join(HOLD_LOG_DIR))?;
    let _guards = acquire_resources(root, &sorted)?;
    let started = now();
    let log_path = root.join(HOLD_LOG_DIR).join(format!(
        "hold-{}-{}.log",
        started,
        std::process::id()
    ));
    let log = Arc::new(Mutex::new(fs::File::create(&log_path)?));
    let mut child = Command::new(&cmd[0])
        .args(&cmd[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let out_handle = {
        let log = log.clone();
        let mut src = child.stdout.take().unwrap();
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
                    let _ = io::stdout().write_all(&buf[..n]);
                }
            }
        })
    };
    let err_handle = {
        let log = log.clone();
        let mut src = child.stderr.take().unwrap();
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
                    let _ = io::stderr().write_all(&buf[..n]);
                }
            }
        })
    };
    let status = child.wait()?;
    let _ = out_handle.join();
    let _ = err_handle.join();
    let ended = now();
    let exit_code = status.code().unwrap_or(-1);
    let payload = fs::read(&log_path)?;
    let output_digest = blake3::hash(&payload).to_hex().to_string();
    let command_line = cmd.join(" ");
    let intent = format!(
        "hold resources={} exit={exit_code} cmd={command_line}",
        sorted.join(",")
    );
    let target = if job.is_empty() {
        command_line.clone()
    } else {
        job.to_string()
    };
    let event = ledger::append_action(root, holder, HOLD_ACTION, &target, &intent, &payload)?;
    let hold_id = ledger::hex(&event.hash);
    let record = HoldRecord {
        hold_id: hold_id.clone(),
        resources: sorted,
        holder: holder.to_string(),
        command: cmd.to_vec(),
        command_line,
        started,
        ended,
        exit_status: exit_code,
        output_digest,
        ledger_index: event.index,
        ledger_hash: ledger::hex(&event.hash),
    };
    let text = serde_json::to_string_pretty(&record)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    fs::write(root.join(HOLD_DIR).join(format!("{hold_id}.json")), format!("{text}\n"))?;
    Ok(HoldOutcome {
        record,
        exit_code,
    })
}

/// Quiet helper for tests: write a hold record plus its ledger event.
#[cfg(test)]
pub fn fake_hold(store: &Path, resource: &str, command_line: &str, output: &[u8]) -> HoldRecord {
    let event = ledger::append_action(
        store,
        "tester",
        HOLD_ACTION,
        command_line,
        "hold test",
        output,
    )
    .unwrap();
    let hold_id = ledger::hex(&event.hash);
    let record = HoldRecord {
        hold_id: hold_id.clone(),
        resources: vec![resource.to_string()],
        holder: "tester".to_string(),
        command: command_line.split_whitespace().map(str::to_string).collect(),
        command_line: command_line.to_string(),
        started: 1,
        ended: 2,
        exit_status: 0,
        output_digest: blake3::hash(output).to_hex().to_string(),
        ledger_index: event.index,
        ledger_hash: ledger::hex(&event.hash),
    };
    fs::create_dir_all(store.join(HOLD_DIR)).unwrap();
    let text = serde_json::to_string_pretty(&record).unwrap();
    fs::write(store.join(HOLD_DIR).join(format!("{hold_id}.json")), format!("{text}\n")).unwrap();
    record
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::LeaseRef;
    use crate::evidence::fixtures::{sample, sealed};
    use crate::evidence::{store_receipt, Mutation, Tier, Verdict};

    fn machine_receipt(lease: Option<LeaseRef>, output_digest: &str) -> Receipt {
        let mut r = sample();
        r.tier = Tier::Machine1Attended;
        r.env_class = "MACHINE1_ATTENDED".to_string();
        r.machine = "machine-1".to_string();
        r.procedure = "scripts/seed0b_machine1.sh".to_string();
        r.declared_mutation = Mutation::RemovableMediaOnly;
        r.observed_mutation = Mutation::RemovableMediaOnly;
        r.output_digest = output_digest.to_string();
        r.lease = lease;
        sealed(r)
    }

    #[test]
    fn bad_resource_names_rejected() {
        assert!(check_resource_name("machine-1").is_ok());
        assert!(check_resource_name("Machine-1").is_err());
        assert!(check_resource_name("../x").is_err());
        assert!(check_resource_name("").is_err());
    }

    #[test]
    fn lease_binding_verifies_end_to_end() {
        let store = crate::testutil::temp_dir("lease-ok");
        let output = b"machine1 run output";
        let hold = fake_hold(&store, "machine-1", "scripts/seed0b_machine1.sh", output);
        let digest = blake3::hash(output).to_hex().to_string();
        let r = machine_receipt(
            Some(LeaseRef {
                hold_id: hold.hold_id.clone(),
                resource: "machine-1".to_string(),
            }),
            &digest,
        );
        store_receipt(&store, &r).unwrap();
        let report = check_lease_ref(&r, &store).unwrap().unwrap();
        assert_eq!(report.status, Verdict::Pass);
    }

    #[test]
    fn hardware_receipt_without_hold_is_incomplete() {
        let store = crate::testutil::temp_dir("lease-missing");
        let r = machine_receipt(None, &"c".repeat(64));
        let report = lease_status(&r, &store).unwrap();
        assert_eq!(report.status, Verdict::Incomplete);
    }

    #[test]
    fn output_digest_mismatch_fails_binding() {
        let store = crate::testutil::temp_dir("lease-digest");
        let output = b"real output";
        let hold = fake_hold(&store, "machine-1", "scripts/seed0b_machine1.sh", output);
        let r = machine_receipt(
            Some(LeaseRef {
                hold_id: hold.hold_id,
                resource: "machine-1".to_string(),
            }),
            &"d".repeat(64),
        );
        let report = check_lease_ref(&r, &store).unwrap().unwrap();
        assert_eq!(report.status, Verdict::Fail);
    }

    #[test]
    fn wrong_resource_fails_binding() {
        let store = crate::testutil::temp_dir("lease-resource");
        let output = b"real output";
        let hold = fake_hold(&store, "machine-1", "scripts/seed0b_machine1.sh", output);
        let digest = blake3::hash(output).to_hex().to_string();
        let r = machine_receipt(
            Some(LeaseRef {
                hold_id: hold.hold_id,
                resource: "machine-2".to_string(),
            }),
            &digest,
        );
        let report = check_lease_ref(&r, &store).unwrap().unwrap();
        assert_eq!(report.status, Verdict::Fail);
    }

    #[test]
    fn missing_hold_record_fails_closed() {
        let store = crate::testutil::temp_dir("lease-gone");
        let r = machine_receipt(
            Some(LeaseRef {
                hold_id: "e".repeat(64),
                resource: "machine-1".to_string(),
            }),
            &"c".repeat(64),
        );
        assert!(check_lease_ref(&r, &store).is_err());
    }
}
