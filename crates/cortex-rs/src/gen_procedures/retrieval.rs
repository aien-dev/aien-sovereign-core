//! Byte-exact reimplementation of omega's file-backed GEN retrieval provider, contract
//! `genret-cli-v3` (tools/gen/retrieval/CONTRACT.md sha256 ee21d714...a3ac, genret.c at gen/retrieval
//! a26be577). Given the same record set and query it produces the same `selected.tsv`,
//! `excluded.tsv`, `suffix.txt`, `query.txt`, `receipt.txt` and `SHA256SUMS` bytes. Every rule is
//! integer arithmetic or byte comparison; nothing here reads a clock, the environment or the network.
//!
//! INTEROP ONLY. This is not the GEN experiment provider and its output must not be passed off as
//! genret output. The validator that runs is the in-process Rust port of recheck.c
//! ([`super::record::validate`]); the recheck binary genret executes, and genret's check of that
//! binary's sha256 against its trust pin, are absent. The caller cannot name any validator digest
//! but [`PINNED_RECHECK_SHA256`], and every receipt starts with
//! `provider=cortex-interop`, is tagged `contract=cortex-interop-v1` (not `genret-cli-v3`) and says
//! `recheck_source_sha256=unverified`. Apart from those marker
//! lines (and the receipt digests that cover them) the six outputs equal genret's byte for byte.
//!
//! The record bytes reach this module through a loader (the Cortex store in `store.rs`); the
//! validation that genret delegates to `recheck` is done in-process by [`super::record::validate`].
//! Record text is only ever copied into the suffix as inert bytes inside a fenced block: the fields
//! that steer eligibility (`id kind split family validity`) are read from the record header only, and
//! the header ends at the first `body` line.

use std::collections::BTreeMap;

use super::record::{self, ReceiptSource};
use super::sha256_hex;

/// The genret contract this module reproduces (the rules, not the receipt tag).
pub const CONTRACT: &str = "genret-cli-v3";
/// The `contract=` value of every interop receipt: never genret's own tag.
pub const INTEROP_CONTRACT: &str = "cortex-interop-v1";
/// sha256 of the recheck.c whose rules [`super::record::validate`] implements (omega gen/abstract
/// 937b033c, the `GENRET_RECHECK_SHA256` pin of mk/gen-retrieval.mk at a26be577). Written to every
/// receipt as `recheck_source_sha256`, exactly as genret compiles it in.
pub const RECHECK_SOURCE_SHA256: &str =
    "05cec285f7b7b2be68b1db1287661513e7e4715da44909065bdd0829daac02b6";
/// The one `recheck_sha256` this adapter accepts: the reference recheck binary built from
/// [`RECHECK_SOURCE_SHA256`] with the mk/gen-retrieval.mk flags (gcc 13.3.0, aarch64; see the fixture
/// PINS), on which the Rust validator was proved verdict-identical. Any other value is refused
/// (`recheck_unpinned`).
pub const PINNED_RECHECK_SHA256: &str =
    "41679de1f1d3cb78d179c418c0df4de174f0556b7c701ebb5af602e602970518";
/// First receipt line of every output: this is Cortex interop output, never a genret receipt.
pub const PROVIDER_MARKER: &str = "provider=cortex-interop";
/// What the receipt says for `recheck_source_sha256`: no binary built from the pinned source ran
/// (as omega's `genret_anyrecheck` test build says).
pub const RECHECK_SOURCE_CLAIM: &str = "unverified";
/// The heading before the first record block, the same for both kinds (CONTRACT.md 6, 66 bytes).
pub const HEADING: &str = "\nVerified reference code from earlier tasks, most relevant first.\n";
const MAX_RECORD: usize = 65536;
const MAX_RECORDS: usize = 65536;
const MAX_TASKFILE: usize = 4096;
const MAX_EXCL: usize = 256;

/// Record kind: arm C (`raw`) or arm D (`abstraction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Raw,
    Abstraction,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Raw => "raw",
            Kind::Abstraction => "abstraction",
        }
    }
}

/// Selection mode (CONTRACT.md section 6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Irrelevant,
    Empty,
    LengthMatched { target_bytes: u32 },
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Irrelevant => "irrelevant",
            Mode::Empty => "empty",
            Mode::LengthMatched { .. } => "length-matched",
        }
    }
}

/// Which task file is being read; it picks genret's refusal word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskRole {
    Requirement,
    Prompt,
}

impl TaskRole {
    fn reason(self) -> &'static str {
        match self {
            TaskRole::Requirement => "requirement_invalid",
            TaskRole::Prompt => "prompt_invalid",
        }
    }
}

/// A public task file that passed the `hidden` path check. The only way to build one is
/// [`read_public_task_file`], so every query input has been through it (the check is not opt-in).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicTaskFile {
    bytes: Vec<u8>,
}

impl PublicTaskFile {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

// open(2) flags for the task reader: never block on a FIFO before the regular-file check, never
// acquire a controlling terminal. Values of the generic Linux ABI (asm-generic/fcntl.h, also x86).
#[cfg(all(
    target_os = "linux",
    any(
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv64"
    )
))]
const TASK_OPEN_FLAGS: i32 = 0o4000 /* O_NONBLOCK */ | 0o400 /* O_NOCTTY */;

/// Reads a requirement or prompt like genret v3 (CONTRACT.md 3): open once; resolve the path of THAT
/// open file through /proc/self/fd and refuse it if any component is exactly `hidden`
/// (`hidden_path`); refuse anything but a regular file; read at most 4096 + 1 bytes from the same
/// descriptor (the query refuses the extra byte). A symlink swapped in after the check changes
/// nothing. As in genret, a hard link to a hidden file placed outside `hidden/` is not detectable.
#[cfg(all(
    target_os = "linux",
    any(
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv64"
    )
))]
pub fn read_public_task_file(
    path: &std::path::Path,
    role: TaskRole,
) -> Result<PublicTaskFile, GenRetError> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::io::AsRawFd;
    let why = role.reason();
    let f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(TASK_OPEN_FLAGS)
        .open(path)
        .map_err(|e| GenRetError::new(why, format!("cannot read {}: {}", path.display(), e)))?;
    let rp = std::fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd()))
        .map_err(|e| GenRetError::new(why, format!("cannot resolve {}: {}", path.display(), e)))?;
    if rp.components().any(|c| c.as_os_str() == "hidden") {
        return Err(GenRetError::new(
            "hidden_path",
            format!("{} is under a 'hidden' directory", path.display()),
        ));
    }
    let md = f
        .metadata()
        .map_err(|e| GenRetError::new(why, format!("cannot stat {}: {}", path.display(), e)))?;
    if !md.is_file() {
        return Err(GenRetError::new(
            why,
            format!("{} is not a regular file", path.display()),
        ));
    }
    let mut bytes = Vec::with_capacity(MAX_TASKFILE + 1);
    f.take(MAX_TASKFILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| GenRetError::new(why, format!("cannot read {}: {}", path.display(), e)))?;
    Ok(PublicTaskFile { bytes })
}

/// Other targets: the reader needs Linux's /proc/self/fd path check and the generic-ABI open(2)
/// flags above, so it refuses with the role's reason instead of failing the build.
#[cfg(not(all(
    target_os = "linux",
    any(
        target_arch = "x86_64",
        target_arch = "x86",
        target_arch = "aarch64",
        target_arch = "arm",
        target_arch = "riscv64"
    )
)))]
pub fn read_public_task_file(
    path: &std::path::Path,
    role: TaskRole,
) -> Result<PublicTaskFile, GenRetError> {
    Err(GenRetError::new(
        role.reason(),
        format!(
            "cannot read {}: the task-file reader is supported on Linux only",
            path.display()
        ),
    ))
}

/// One query: the public requirement and prompt plus the genret options.
#[derive(Debug, Clone)]
pub struct GenQuery<'a> {
    pub requirement: &'a PublicTaskFile,
    pub prompt: &'a PublicTaskFile,
    pub kind: Kind,
    pub k: u32,
    pub cap_bytes: u32,
    pub mode: Mode,
    pub exclude_families: Vec<String>,
    /// genret `--recheck-sha256`. Cortex runs no recheck binary, so the only value accepted is
    /// [`PINNED_RECHECK_SHA256`], the reference build whose verdicts the in-process port of recheck.c
    /// was proved to reproduce; anything else is refused with `recheck_unpinned`.
    pub recheck_sha256: &'a str,
}

/// A fatal condition (genret exit 2). `reason` is the CONTRACT.md section 7 word, or one of the
/// Cortex-side words `store_unreadable` / `space_invalid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenRetError {
    pub reason: &'static str,
    pub detail: String,
}

impl GenRetError {
    pub(crate) fn new(reason: &'static str, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for GenRetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "genret: FATAL {} {}", self.reason, self.detail)
    }
}

impl std::error::Error for GenRetError {}

/// A selected record (one `selected.tsv` row). Built only inside `gen_procedures`; read-only outside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    pub(super) rank: usize,
    pub(super) id: String,
    pub(super) digest: String,
    pub(super) score: u64,
    pub(super) bytes: usize,
    pub(super) pad: bool,
}

impl Selected {
    pub fn rank(&self) -> usize {
        self.rank
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn score(&self) -> u64 {
        self.score
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    pub fn pad(&self) -> bool {
        self.pad
    }
}

/// A record that was not selected (one `excluded.tsv` row). `score` is None when not indexed.
/// Built only inside `gen_procedures`; read-only outside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Excluded {
    pub(super) id: String,
    pub(super) digest: String,
    pub(super) score: Option<u64>,
    pub(super) reason: String,
}

impl Excluded {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn score(&self) -> Option<u64> {
        self.score
    }
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// The interop outputs, plus parsed rows. Fields are private and the value is built only inside
/// `gen_procedures`, so a caller cannot strip the INTEROP marker lines and rebuild a receipt that
/// passes as genret's; everything is exposed through read accessors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenRetOutput {
    selected_tsv: Vec<u8>,
    excluded_tsv: Vec<u8>,
    suffix: Vec<u8>,
    query_txt: Vec<u8>,
    receipt_txt: Vec<u8>,
    sha256sums: Vec<u8>,
    selected: Vec<Selected>,
    excluded: Vec<Excluded>,
    /// Records excluded because Cortex holds a revocation for them. genret has no revocations, so
    /// when this is non-zero the output is NOT comparable with the file provider on the same set.
    revoked: usize,
}

impl GenRetOutput {
    /// The six files genret writes into `--out`, in its order.
    pub fn files(&self) -> [(&'static str, &[u8]); 6] {
        [
            ("selected.tsv", &self.selected_tsv),
            ("excluded.tsv", &self.excluded_tsv),
            ("suffix.txt", &self.suffix),
            ("query.txt", &self.query_txt),
            ("receipt.txt", &self.receipt_txt),
            ("SHA256SUMS", &self.sha256sums),
        ]
    }
    pub fn selected_tsv(&self) -> &[u8] {
        &self.selected_tsv
    }
    pub fn excluded_tsv(&self) -> &[u8] {
        &self.excluded_tsv
    }
    pub fn suffix(&self) -> &[u8] {
        &self.suffix
    }
    pub fn query_txt(&self) -> &[u8] {
        &self.query_txt
    }
    pub fn receipt_txt(&self) -> &[u8] {
        &self.receipt_txt
    }
    pub fn sha256sums(&self) -> &[u8] {
        &self.sha256sums
    }
    pub fn selected(&self) -> &[Selected] {
        &self.selected
    }
    pub fn excluded(&self) -> &[Excluded] {
        &self.excluded
    }
    /// How many records a Cortex revocation excluded.
    pub fn revoked(&self) -> usize {
        self.revoked
    }

    /// True when the output can be compared byte for byte with genret on the same record set.
    pub fn parity_comparable(&self) -> bool {
        self.revoked == 0
    }
}

/// One record as the loader returns it.
pub(crate) struct Loaded<'a> {
    pub bytes: Vec<u8>,
    pub receipts: Box<dyn ReceiptSource + 'a>,
    pub revoked: bool,
}

/// One manifest line.
pub(crate) struct ManifestLine {
    pub digest: String,
    pub path: String,
}

fn c_name(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-'
}
fn c_word(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}
pub(crate) fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
/// `--exclude-family` / record family token: [a-z0-9_-]{1,64}
pub(crate) fn family_token_ok(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.bytes().all(c_name)
}

/// Parses and checks a MANIFEST exactly as genret does (digest first, then the line rules).
pub(crate) fn parse_manifest(man: &[u8], pin: &str) -> Result<Vec<ManifestLine>, GenRetError> {
    let mdig = sha256_hex(man);
    if mdig != pin {
        return Err(GenRetError::new(
            "manifest_digest_mismatch",
            format!("MANIFEST has sha256 {}, pinned {}", mdig, pin),
        ));
    }
    if man.is_empty() || man[man.len() - 1] != b'\n' {
        return Err(GenRetError::new(
            "manifest_malformed",
            "empty or no final newline",
        ));
    }
    let nr = man.iter().filter(|&&c| c == b'\n').count();
    if nr > MAX_RECORDS {
        return Err(GenRetError::new(
            "manifest_malformed",
            format!("more than {} lines", MAX_RECORDS),
        ));
    }
    let mut out: Vec<ManifestLine> = Vec::with_capacity(nr);
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (li, line) in man[..man.len() - 1].split(|&c| c == b'\n').enumerate() {
        let n = line.len();
        let bad_head = n < 67
            || !line[..64]
                .iter()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
            || line[64] != b' '
            || line[65] != b' ';
        if bad_head {
            return Err(GenRetError::new(
                "manifest_malformed",
                format!("line {} is not '<sha256>  <path>'", li + 1),
            ));
        }
        if line.contains(&0) {
            return Err(GenRetError::new(
                "manifest_malformed",
                format!("line {} holds NUL", li + 1),
            ));
        }
        let path = &line[66..];
        if path[0] == b'/' || path.len() > 4096 {
            return Err(GenRetError::new(
                "manifest_malformed",
                format!("line {}: bad path", li + 1),
            ));
        }
        for comp in path.split(|&c| c == b'/') {
            if comp.is_empty() || comp == b"." || comp == b".." {
                return Err(GenRetError::new(
                    "manifest_malformed",
                    format!("line {}: bad path component", li + 1),
                ));
            }
            if !comp
                .iter()
                .all(|&c| c_name(c) || c.is_ascii_uppercase() || c == b'.')
            {
                return Err(GenRetError::new(
                    "manifest_malformed",
                    format!("line {}: bad path byte", li + 1),
                ));
            }
        }
        // checked above: hex + ASCII path bytes only
        let path = String::from_utf8_lossy(path).into_owned();
        if !seen.insert(path.clone()) {
            return Err(GenRetError::new(
                "manifest_malformed",
                format!("duplicate path {}", path),
            ));
        }
        out.push(ManifestLine {
            digest: String::from_utf8_lossy(&line[..64]).into_owned(),
            path,
        });
    }
    Ok(out)
}

fn lowered(b: &[u8]) -> Vec<u8> {
    b.iter().map(u8::to_ascii_lowercase).collect()
}

/// CONTRACT.md 4: maximal runs of [a-z0-9] in the lowered bytes.
fn tokenize(low: &[u8], out: &mut Vec<Vec<u8>>) {
    for t in low.split(|&c| !c_word(c)) {
        if !t.is_empty() {
            out.push(t.to_vec());
        }
    }
}

/// CONTRACT.md 6, fixed-point log2 in units of 2^-16.
pub(crate) fn log2q16(a: u64, mut b: u64) -> u64 {
    let mut k = 0u64;
    while a >= 2 * b {
        b *= 2;
        k += 1;
    }
    let mut y = (a << 30) / b;
    let mut r = k << 16;
    for i in (0..16).rev() {
        y = (y * y) >> 30;
        if y >= 1u64 << 31 {
            y >>= 1;
            r += 1u64 << i;
        }
    }
    r
}

/// The record fields genret reads, taken from a record that already passed [`record::validate`].
struct Rec {
    digest: String,
    id: String,
    kind: String,
    split: String,
    families: Vec<String>,
    validity: String,
    entry: String,
    params: String,
    ret: String,
    desc: String,
    body: Vec<u8>,
    revoked: bool,
    reason: String,
    indexed: bool,
    selected: bool,
    pad: bool,
    tf: BTreeMap<Vec<u8>, u32>,
    dl: u64,
    score: u64,
}

pub(crate) fn header_fields(bytes: &[u8]) -> Result<(BTreeMap<String, String>, Vec<u8>), String> {
    let mut fields = BTreeMap::new();
    let mut pos = 0;
    loop {
        let nl = match bytes[pos..].iter().position(|&c| c == b'\n') {
            Some(p) => pos + p,
            None => return Err("no 'body' line".into()),
        };
        let line = std::str::from_utf8(&bytes[pos..nl]).map_err(|_| "header not ASCII")?;
        pos = nl + 1;
        if line == "body" {
            break;
        }
        let (k, v) = line.split_once(' ').unwrap_or((line, ""));
        if matches!(
            k,
            "id" | "kind" | "split" | "family" | "validity" | "entry" | "params" | "ret" | "desc"
        ) && fields.insert(k.to_string(), v.to_string()).is_some()
        {
            return Err(format!("key {} repeated", k));
        }
    }
    Ok((fields, bytes[pos..].to_vec()))
}

/// One heading for both kinds: C and D suffixes differ only in record content (v3).
fn render_head(b: &mut Vec<u8>) {
    b.extend_from_slice(HEADING.as_bytes());
}

fn render_block(b: &mut Vec<u8>, i: usize, r: &Rec) {
    b.extend_from_slice(format!("\nRecord {}.\n\nentry ", i).as_bytes());
    b.extend_from_slice(r.entry.as_bytes());
    b.extend_from_slice(b"\nparams");
    if !r.params.is_empty() {
        b.push(b' ');
        b.extend_from_slice(r.params.as_bytes());
    }
    b.extend_from_slice(b"\nret ");
    b.extend_from_slice(r.ret.as_bytes());
    b.extend_from_slice(b"\n\n");
    b.extend_from_slice(r.desc.as_bytes());
    b.extend_from_slice(b"\n\n```\n");
    b.extend_from_slice(&r.body);
    b.extend_from_slice(b"```\n");
}

fn block_len(i: usize, r: &Rec) -> usize {
    let mut b = Vec::new();
    render_block(&mut b, i, r);
    b.len()
}

/// The fields the eligibility chain reads from one record.
pub(crate) struct Eligibility<'a> {
    /// `Some(kind)` = the query's `--kind`; `None` = no kind filter (the audit).
    pub kind: Option<&'a str>,
    pub rec_kind: &'a str,
    pub split: &'a str,
    pub validity: &'a str,
    pub revoked: bool,
    pub families: &'a [String],
    pub exclude: &'a [String],
    pub body: &'a [u8],
    pub desc: &'a [u8],
}

/// CONTRACT.md 5, in order, with Cortex's `revoked` after validity: the first failing reason, or
/// None when the record is indexed. The one chain both the query and the audit use.
pub(crate) fn ineligible(e: &Eligibility<'_>) -> Option<String> {
    if let Some(k) = e.kind {
        if e.rec_kind != k {
            return Some(format!("kind:{}", e.rec_kind));
        }
    }
    if e.split != "corpus" {
        return Some(format!("split:{}", e.split));
    }
    if e.validity != "verified" {
        return Some(format!("validity:{}", e.validity));
    }
    if e.revoked {
        return Some("revoked".into());
    }
    for fam in e.families {
        if let Some(x) = e.exclude.iter().find(|x| *x == fam) {
            return Some(format!("family_excluded:{}", x));
        }
    }
    if contains3(e.body) || contains3(e.desc) {
        return Some("poisoned:fence".into());
    }
    None
}

pub(crate) fn contains3(h: &[u8]) -> bool {
    h.windows(3).any(|w| w == b"```")
}

/// Checks the query options and task bytes exactly as genret does, and returns the sorted distinct
/// query terms.
pub(crate) fn check_query(q: &GenQuery<'_>, pin: &str) -> Result<Vec<Vec<u8>>, GenRetError> {
    let usage = |why: &str| Err(GenRetError::new("usage", why.to_string()));
    if !(1..=64).contains(&q.k) || !(1..=1_048_576).contains(&q.cap_bytes) {
        return usage("number out of range");
    }
    if q.exclude_families.len() > MAX_EXCL {
        return usage("too many --exclude-family");
    }
    if q.exclude_families.iter().any(|f| !family_token_ok(f)) {
        return usage("--exclude-family must match [a-z0-9_-]{1,64}");
    }
    if !is_hex64(pin) {
        return usage("--manifest-sha256 must be 64 lowercase hex");
    }
    if !is_hex64(q.recheck_sha256) {
        return usage("--recheck-sha256 must be 64 lowercase hex");
    }
    if q.recheck_sha256 != PINNED_RECHECK_SHA256 {
        return Err(GenRetError::new(
            "recheck_unpinned",
            format!(
                "recheck_sha256 {} is not the pinned reference {}",
                q.recheck_sha256, PINNED_RECHECK_SHA256
            ),
        ));
    }
    if let Mode::LengthMatched { target_bytes } = q.mode {
        if !(1..=1_048_576).contains(&target_bytes) {
            return usage("number out of range");
        }
        if target_bytes > q.cap_bytes {
            return usage("--target-bytes must not exceed --cap-bytes");
        }
    }
    let (req, pr) = (q.requirement.bytes(), q.prompt.bytes());
    if req.len() > MAX_TASKFILE || req.contains(&0) {
        return Err(GenRetError::new(
            "requirement_invalid",
            format!("requirement: over {} bytes or holds NUL", MAX_TASKFILE),
        ));
    }
    if pr.is_empty() || pr.len() > MAX_TASKFILE {
        return Err(GenRetError::new(
            "prompt_invalid",
            format!("prompt: must be 1..{} bytes", MAX_TASKFILE),
        ));
    }
    if pr
        .iter()
        .any(|&c| c != b'\n' && !(0x20..=0x7e).contains(&c))
    {
        return Err(GenRetError::new(
            "prompt_invalid",
            "prompt: byte outside \\n and 0x20..0x7E",
        ));
    }
    let reql = lowered(req);
    let mut terms: Vec<Vec<u8>> = Vec::new();
    let qk: [&[u8]; 3] = [b"entry", b"params", b"ret"];
    let mut seen = [0u32; 3];
    let mut i = 0;
    while i < req.len() {
        let e = req[i..]
            .iter()
            .position(|&c| c == b'\n')
            .map(|p| i + p)
            .unwrap_or(req.len());
        for (j, k) in qk.iter().enumerate() {
            let kl = k.len();
            if e - i >= kl && &req[i..i + kl] == *k && (e - i == kl || req[i + kl] == b' ') {
                seen[j] += 1;
                let vs = i + kl + usize::from(e - i > kl);
                tokenize(&reql[vs..e], &mut terms);
            }
        }
        i = e + 1;
    }
    for (j, k) in qk.iter().enumerate() {
        if seen[j] != 1 {
            return Err(GenRetError::new(
                "requirement_invalid",
                format!(
                    "requirement: needs exactly one '{}' line",
                    String::from_utf8_lossy(k)
                ),
            ));
        }
    }
    tokenize(&lowered(pr), &mut terms);
    terms.sort();
    terms.dedup();
    Ok(terms)
}

/// Receipts read from a record set's `receipts/` directory (what genret passes to recheck).
pub(crate) struct DirReceipts(pub std::path::PathBuf);
impl ReceiptSource for DirReceipts {
    fn receipt(&self, id: &str) -> Option<Vec<u8>> {
        std::fs::read(self.0.join(id)).ok()
    }
}

/// genret over a record set on disk (`--corpus <MANIFEST>`), with the in-process validator in place of
/// the recheck binary. Read-only: it writes nothing and returns the six outputs. This is the
/// file-backed reference the Cortex store is compared with, including on refused record sets.
pub fn query_record_files(
    manifest: &std::path::Path,
    pin: &str,
    q: &GenQuery<'_>,
) -> Result<GenRetOutput, GenRetError> {
    check_query(q, pin)?;
    let man = std::fs::read(manifest).map_err(|e| {
        GenRetError::new(
            "manifest_unreadable",
            format!("{}: {}", manifest.display(), e),
        )
    })?;
    let dir = manifest
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    run_query(&man, pin, q, |m| {
        let bytes = std::fs::read(dir.join(&m.path))
            .map_err(|_| GenRetError::new("record_missing", m.path.clone()))?;
        Ok(Loaded {
            bytes,
            receipts: Box::new(DirReceipts(dir.join("receipts"))),
            revoked: false,
        })
    })
}

/// Runs one genret query over a record set. `manifest` is the MANIFEST bytes, `pin` the frozen
/// record-set digest, `load(path)` returns the record stored for a manifest path.
pub(crate) fn run_query<'a>(
    manifest: &[u8],
    pin: &str,
    q: &GenQuery<'_>,
    mut load: impl FnMut(&ManifestLine) -> Result<Loaded<'a>, GenRetError>,
) -> Result<GenRetOutput, GenRetError> {
    let terms = check_query(q, pin)?;
    let lines = parse_manifest(manifest, pin)?;

    // records: integrity is fatal (missing, oversize, stale digest), then the recheck rules
    let mut loaded = Vec::with_capacity(lines.len());
    for m in &lines {
        let l = load(m)?;
        if l.bytes.len() > MAX_RECORD {
            return Err(GenRetError::new(
                "invalid_record",
                format!("{}: over {} bytes", m.path, MAX_RECORD),
            ));
        }
        let d = sha256_hex(&l.bytes);
        if d != m.digest {
            return Err(GenRetError::new(
                "stale_digest",
                format!("{} has sha256 {}, manifest says {}", m.path, d, m.digest),
            ));
        }
        loaded.push(l);
    }
    let mut recs: Vec<Rec> = Vec::with_capacity(lines.len());
    for (m, l) in lines.iter().zip(&loaded) {
        let meta = record::validate(&l.bytes, None, Some(l.receipts.as_ref())).map_err(|r| {
            GenRetError::new("invalid_record", format!("recheck: {}: {}", m.path, r))
        })?;
        let (f, body) = header_fields(&l.bytes)
            .map_err(|e| GenRetError::new("invalid_record", format!("{}: {}", m.path, e)))?;
        let get = |k: &str| f.get(k).cloned().unwrap_or_default();
        if get("id") != meta.id {
            return Err(GenRetError::new(
                "invalid_record",
                format!("{}: id differs between parsers", m.path),
            ));
        }
        recs.push(Rec {
            digest: m.digest.clone(),
            id: meta.id,
            kind: meta.kind,
            split: meta.split,
            families: meta.families,
            validity: meta.validity,
            entry: get("entry"),
            params: get("params"),
            ret: get("ret"),
            desc: get("desc"),
            body,
            revoked: l.revoked,
            reason: String::new(),
            indexed: false,
            selected: false,
            pad: false,
            tf: BTreeMap::new(),
            dl: 0,
            score: 0,
        });
    }
    drop(loaded);
    let mut byid: Vec<usize> = (0..recs.len()).collect();
    byid.sort_by(|&a, &b| recs[a].id.as_bytes().cmp(recs[b].id.as_bytes()));
    for w in byid.windows(2) {
        if recs[w[0]].id == recs[w[1]].id {
            return Err(GenRetError::new("duplicate_id", recs[w[1]].id.clone()));
        }
    }

    // eligibility (CONTRACT.md 5); `revoked` is the one Cortex-side addition, after validity
    let kind = q.kind.as_str();
    let mut n_idx: u64 = 0;
    let mut total_t: u64 = 0;
    let mut revoked = 0usize;
    for r in recs.iter_mut() {
        if r.kind == kind && r.split == "corpus" && r.validity == "verified" && r.revoked {
            revoked += 1;
        }
        r.reason = ineligible(&Eligibility {
            kind: Some(kind),
            rec_kind: &r.kind,
            split: &r.split,
            validity: &r.validity,
            revoked: r.revoked,
            families: &r.families,
            exclude: &q.exclude_families,
            body: &r.body,
            desc: r.desc.as_bytes(),
        })
        .unwrap_or_default();
        if !r.reason.is_empty() {
            continue;
        }
        r.indexed = true;
        n_idx += 1;
        let mut toks = Vec::new();
        for v in [&r.entry, &r.params, &r.ret, &r.desc] {
            tokenize(&lowered(v.as_bytes()), &mut toks);
        }
        tokenize(&lowered(&r.body), &mut toks);
        r.dl = toks.len() as u64;
        total_t += r.dl;
        for t in toks {
            *r.tf.entry(t).or_insert(0) += 1;
        }
    }

    // scoring (CONTRACT.md 6)
    for t in &terms {
        let df = recs
            .iter()
            .filter(|r| r.indexed && r.tf.contains_key(t))
            .count() as u64;
        if df == 0 {
            continue;
        }
        let idf = log2q16(2 * n_idx + 2, 2 * df + 1);
        for r in recs.iter_mut().filter(|r| r.indexed) {
            let tf = match r.tf.get(t) {
                Some(&c) => c as u128,
                None => continue,
            };
            let (tt, dl, n) = (total_t as u128, r.dl as u128, n_idx as u128);
            let num = idf as u128 * tf * 44 * tt;
            let den = 20 * tt * tf + 6 * tt + 18 * dl * n;
            r.score += (num / den) as u64;
        }
    }

    // ranking and selection
    let mut ord: Vec<usize> = (0..recs.len()).filter(|&i| recs[i].indexed).collect();
    let irr = q.mode == Mode::Irrelevant;
    ord.sort_by(|&a, &b| {
        let (x, y) = (&recs[a], &recs[b]);
        let s = if irr {
            x.score.cmp(&y.score)
        } else {
            y.score.cmp(&x.score)
        };
        s.then_with(|| x.id.as_bytes().cmp(y.id.as_bytes()))
    });
    let hlen = HEADING.len();
    let mut sel: Vec<usize> = Vec::new();
    let mut total = 0usize;
    let mut unpadded = 0usize;
    let mut npad = 0usize;
    let mut pad_reason = "-";
    let (k, cap) = (q.k as usize, q.cap_bytes as usize);
    if q.mode == Mode::Empty {
        for &i in &ord {
            recs[i].reason = "ablation_empty".into();
        }
    } else {
        let mut stop: Option<&'static str> = None;
        for &i in &ord {
            if !irr && recs[i].score == 0 {
                recs[i].reason = "no_match".into();
                continue;
            }
            if stop.is_none() {
                if sel.len() == k {
                    stop = Some("beyond_k");
                } else {
                    let add =
                        block_len(sel.len() + 1, &recs[i]) + if sel.is_empty() { hlen } else { 0 };
                    if total + add > cap {
                        stop = Some("cap_bytes");
                    } else {
                        total += add;
                        recs[i].selected = true;
                        sel.push(i);
                        continue;
                    }
                }
            }
            recs[i].reason = stop.unwrap_or_default().to_string();
        }
        // length-matched (v3): leave a selection at or over the target alone (c_not_shorter); else
        // append whole records from the rest of the ranking, skipping any that would pass the cap,
        // until the target is reached (reached) or candidates run out (exhausted)
        unpadded = total;
        if let Mode::LengthMatched { target_bytes } = q.mode {
            let target = target_bytes as usize;
            if total >= target {
                pad_reason = "c_not_shorter";
            } else {
                pad_reason = "exhausted";
                for &i in &ord {
                    if total >= target {
                        break;
                    }
                    if recs[i].reason != "beyond_k" && recs[i].reason != "cap_bytes" {
                        continue;
                    }
                    let add =
                        block_len(sel.len() + 1, &recs[i]) + if sel.is_empty() { hlen } else { 0 };
                    if total + add > cap {
                        continue;
                    }
                    total += add;
                    recs[i].selected = true;
                    recs[i].pad = true;
                    recs[i].reason.clear();
                    sel.push(i);
                    npad += 1;
                }
                if total >= target {
                    pad_reason = "reached";
                }
            }
        }
    }

    // outputs (CONTRACT.md 8)
    let mut bs = b"rank\tid\tdigest\tscore\tbytes\trole\n".to_vec();
    let mut bx = Vec::new();
    let mut selected = Vec::new();
    if !sel.is_empty() {
        render_head(&mut bx);
    }
    for (n, &i) in sel.iter().enumerate() {
        let r = &recs[i];
        let before = bx.len();
        render_block(&mut bx, n + 1, r);
        let role = if r.pad { "pad" } else { "top" };
        bs.extend_from_slice(
            format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                n + 1,
                r.id,
                r.digest,
                r.score,
                bx.len() - before,
                role
            )
            .as_bytes(),
        );
        selected.push(Selected {
            rank: n + 1,
            id: r.id.clone(),
            digest: r.digest.clone(),
            score: r.score,
            bytes: bx.len() - before,
            pad: r.pad,
        });
    }
    if bx.len() != total {
        return Err(GenRetError::new(
            "internal",
            format!("suffix length {} != accounted {}", bx.len(), total),
        ));
    }
    let mut be = b"id\tdigest\tscore\treason\n".to_vec();
    let mut excluded = Vec::new();
    for &i in &byid {
        let r = &recs[i];
        if r.selected {
            continue;
        }
        let score = if r.indexed { Some(r.score) } else { None };
        let sc = score.map(|s| s.to_string()).unwrap_or_else(|| "-".into());
        be.extend_from_slice(format!("{}\t{}\t{}\t{}\n", r.id, r.digest, sc, r.reason).as_bytes());
        excluded.push(Excluded {
            id: r.id.clone(),
            digest: r.digest.clone(),
            score,
            reason: r.reason.clone(),
        });
    }
    let mut bq = Vec::new();
    for t in &terms {
        bq.extend_from_slice(t);
        bq.push(b'\n');
    }
    let lm = match q.mode {
        Mode::LengthMatched { target_bytes } => Some(target_bytes as i64),
        _ => None,
    };
    let (pad, unp, gap, target) = match lm {
        Some(t) => (
            if npad > 0 {
                npad.to_string()
            } else {
                "none".to_string()
            },
            unpadded.to_string(),
            (bx.len() as i64 - t).to_string(),
            t.to_string(),
        ),
        None => ("-".into(), "-".into(), "-".into(), "-".into()),
    };
    let excl = if q.exclude_families.is_empty() {
        "-".to_string()
    } else {
        q.exclude_families.join(",")
    };
    // CONTRACT.md 8, the 25 keys in order, behind three interop marker lines (provider,
    // genret_parity, cortex_revoked), tagged contract=cortex-interop-v1 and with recheck_source_sha256=unverified: this receipt can never
    // pass as a genret receipt. genret_parity=no when a Cortex revocation changed the outcome.
    let receipt = format!(
        "{}\ngenret_parity={}\ncortex_revoked={}\ncontract={}\nmode={}\nkind={}\nk={}\ncap_bytes={}\ntarget_bytes={}\nexclude_family={}\n\
         manifest_sha256={}\nrecheck_sha256={}\nrecheck_source_sha256={}\n\
         requirement_sha256={}\nprompt_sha256={}\nquery_sha256={}\n\
         records={}\nindexed={}\ntotal_tokens={}\nselected={}\n\
         pad={}\npad_reason={}\nunpadded_bytes={}\nsuffix_bytes={}\nlength_gap={}\n\
         suffix_sha256={}\nselected_sha256={}\nexcluded_sha256={}\n",
        PROVIDER_MARKER,
        if revoked == 0 { "yes" } else { "no" },
        revoked,
        INTEROP_CONTRACT,
        q.mode.as_str(),
        kind,
        q.k,
        q.cap_bytes,
        target,
        excl,
        pin,
        q.recheck_sha256,
        RECHECK_SOURCE_CLAIM,
        sha256_hex(q.requirement.bytes()),
        sha256_hex(q.prompt.bytes()),
        sha256_hex(&bq),
        recs.len(),
        n_idx,
        total_t,
        sel.len(),
        pad,
        pad_reason,
        unp,
        bx.len(),
        gap,
        sha256_hex(&bx),
        sha256_hex(&bs),
        sha256_hex(&be),
    )
    .into_bytes();
    let mut out = GenRetOutput {
        selected_tsv: bs,
        excluded_tsv: be,
        suffix: bx,
        query_txt: bq,
        receipt_txt: receipt,
        sha256sums: Vec::new(),
        selected,
        excluded,
        revoked,
    };
    let mut sums = Vec::new();
    for (name, b) in out.files().iter().take(5) {
        sums.extend_from_slice(format!("{}  {}\n", sha256_hex(b), name).as_bytes());
    }
    out.sha256sums = sums;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::log2q16;

    #[test]
    fn log2q16_matches_known_points() {
        assert_eq!(log2q16(2, 1), 1 << 16);
        assert_eq!(log2q16(8, 1), 3 << 16);
        assert_eq!(log2q16(5, 5), 0);
        // log2(3) = 1.58496..., in units of 2^-16 = 103872.6...
        let v = log2q16(3, 1);
        assert!((103_870..=103_874).contains(&v), "{}", v);
    }
}
