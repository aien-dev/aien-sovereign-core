//! Strict validator for GEN record schema v1 (`gen-record 1`).
//!
//! Line-for-line port of omega `tools/gen/abstract/recheck.c` (gen/abstract 937b033c, sha256
//! 05cec285...02b6), the validator the schema names (RECORD_SCHEMA.md sha256 9be17b6b...6b53). It
//! checks sections 1 to 3 of the schema, optionally the `source` digests against a repository root,
//! and the record's test receipt. It accepts or refuses with the same line number and reason text as
//! recheck (pinned by `tests/fixtures/gen_records_v1/recheck/verdicts.tsv`). It runs no program.

use std::path::Path;

use super::sha256_hex;

const MAXB: usize = 65536;
const MAXL: usize = 1024;
const MAXLINES: usize = 2048;

/// Why a record was refused: recheck's `line <n>: <reason>` (line 0 = whole file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub line: usize,
    pub reason: String,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.reason)
    }
}

/// Where the validator gets a receipt's bytes from, by its 64-hex id. `None` = missing.
pub trait ReceiptSource {
    fn receipt(&self, id: &str) -> Option<Vec<u8>>;
}

/// No receipt check (recheck without `--receipts`).
pub struct NoReceipts;
impl ReceiptSource for NoReceipts {
    fn receipt(&self, _id: &str) -> Option<Vec<u8>> {
        None
    }
}

/// The provenance fields of an accepted record, read from its verbatim bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordMeta {
    pub id: String,
    pub kind: String,
    pub split: String,
    pub families: Vec<String>,
    pub validity: String,
    pub receipt: String,
    /// (task id, sha256 of its solution.omegese)
    pub ancestry: Vec<(String, String)>,
    /// (repository-relative path, sha256)
    pub sources: Vec<(String, String)>,
}

type R<T> = Result<T, Refusal>;

fn fail<T>(line: usize, reason: String) -> R<T> {
    // recheck formats into a 512-byte buffer
    let mut reason = reason;
    if reason.len() > 511 {
        reason.truncate(511);
    }
    Err(Refusal { line, reason })
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

pub(crate) fn is_taskid(s: &str) -> bool {
    (1..=40).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

const SCALARS: [&str; 9] = [
    "bool", "uchar", "ushort", "uint", "ulong", "byte", "short", "int", "long",
];
const IRNAMES: [&str; 9] = ["bool", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64"];
const RESERVED: [&str; 47] = [
    "fn", "to", "void", "end", "if", "else", "while", "bound", "for", "break", "ret", "put",
    "free", "trap", "add", "sub", "mul", "div", "rem", "and", "or", "xor", "shl", "shr", "eq",
    "ne", "lt", "le", "gt", "ge", "neg", "not", "get", "new", "own", "edit", "view", "true",
    "false", "u8", "u16", "u32", "u64", "i8", "i16", "i32", "i64",
];

fn is_scalar(w: &str) -> bool {
    SCALARS.contains(&w)
}

fn irname(s: &str) -> &'static str {
    SCALARS
        .iter()
        .position(|x| *x == s)
        .map(|i| IRNAMES[i])
        .unwrap_or("?")
}

fn is_ident(w: &str) -> bool {
    let b = w.as_bytes();
    if b.is_empty() || b.len() > 63 {
        return false;
    }
    if !(b[0].is_ascii_alphabetic() || b[0] == b'_') {
        return false;
    }
    if !b[1..]
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || *c == b'_')
    {
        return false;
    }
    !is_scalar(w) && !RESERVED.contains(&w)
}

/// recheck `words`: split on single spaces, at most `max` words (None when more).
fn words(s: &str, max: usize) -> Option<Vec<&str>> {
    if s.is_empty() {
        return Some(Vec::new());
    }
    let w: Vec<&str> = s.split(' ').collect();
    if w.len() > max {
        None
    } else {
        Some(w)
    }
}

fn is_len(w: &str) -> bool {
    let b = w.as_bytes();
    if b.is_empty() || b[0] == b'0' || b.len() > 2 || !b.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let v: u32 = w.parse().unwrap_or(0);
    (1..=64).contains(&v)
}

/// a type at w[i]: scalar (1 word) or view|edit|own scalar len (3 words); returns words used or 0
fn type_at(w: &[&str], i: usize) -> usize {
    if i >= w.len() {
        return 0;
    }
    if is_scalar(w[i]) {
        return 1;
    }
    if matches!(w[i], "view" | "edit" | "own")
        && i + 2 < w.len()
        && is_scalar(w[i + 1])
        && is_len(w[i + 2])
    {
        return 3;
    }
    0
}

struct Param {
    name: String,
    ty: String,
    len: u32,
}

struct Sig {
    name: String,
    params: Vec<Param>,
    ret: String,
}

fn parse_sig(s: &str, line: usize) -> R<Sig> {
    let w = words(s, 64).unwrap_or_default();
    let n = w.len();
    if n < 4 || w[0] != "fn" {
        return fail(
            line,
            "signature is not 'fn <name> (<param> <type>)* to <type>'".into(),
        );
    }
    if !is_ident(w[1]) {
        return fail(
            line,
            format!("function name '{}' is not an identifier", w[1]),
        );
    }
    let mut g = Sig {
        name: w[1].to_string(),
        params: Vec::new(),
        ret: String::new(),
    };
    let mut i = 2;
    while i < n && w[i] != "to" {
        if g.params.len() == 6 {
            return fail(line, "more than 6 parameters".into());
        }
        if !is_ident(w[i]) {
            return fail(
                line,
                format!(
                    "parameter name '{}' is not an identifier (or is reserved)",
                    w[i]
                ),
            );
        }
        if g.params.iter().any(|p| p.name == w[i]) {
            return fail(line, format!("parameter name '{}' repeated", w[i]));
        }
        if w[i] == g.name {
            return fail(
                line,
                format!("parameter '{}' has the function's name", w[i]),
            );
        }
        let t = type_at(&w, i + 1);
        if t == 0 {
            return fail(line, format!("parameter '{}' has no valid type", w[i]));
        }
        let (ty, len) = if t == 1 {
            (w[i + 1].to_string(), 0)
        } else {
            (
                format!("{} {} {}", w[i + 1], w[i + 2], w[i + 3]),
                w[i + 3].parse().unwrap_or(0),
            )
        };
        g.params.push(Param {
            name: w[i].to_string(),
            ty,
            len,
        });
        i += 1 + t;
    }
    if i >= n {
        return fail(line, "signature has no 'to <type>'".into());
    }
    i += 1;
    if i < n && w[i] == "void" && i + 1 == n {
        g.ret = "void".into();
        return Ok(g);
    }
    if type_at(&w, i) != 1 || i + 1 != n {
        return fail(
            line,
            "return type must be one scalar type or void, and end the line".into(),
        );
    }
    g.ret = w[i].to_string();
    Ok(g)
}

fn is_int(w: &str) -> bool {
    let p = w.strip_prefix('-').unwrap_or(w);
    let b = p.as_bytes();
    if b.is_empty() || (b[0] == b'0' && b.len() > 1) || b.len() > 20 {
        return false;
    }
    if w.starts_with('-') && p == "0" {
        return false;
    }
    b.iter().all(u8::is_ascii_digit)
}

/// glibc `atoi` on a non-negative `is_int` word: `(int) strtol(w)`, strtol saturating at LONG_MAX
/// (64-bit), then truncated to int, exactly as recheck sees it.
fn c_atoi(w: &str) -> i32 {
    let v: u128 = w.parse().unwrap_or(0);
    let l: i64 = if v > i64::MAX as u128 {
        i64::MAX
    } else {
        v as i64
    };
    l as i32
}

fn parse_call(s: &str, nargs: usize, trap_ok: bool, line: usize, what: &str) -> R<()> {
    let w = match words(s, 80) {
        Some(w) => w,
        None => return fail(line, format!("{} has too many words", what)),
    };
    let n = w.len();
    let mut a = 0;
    while a < n && w[a] != "->" {
        if !is_int(w[a]) {
            return fail(line, format!("{} argument is not a decimal number", what));
        }
        a += 1;
    }
    if a != nargs {
        return fail(
            line,
            format!(
                "{} has the wrong number of arguments for the signature",
                what
            ),
        );
    }
    if a >= n {
        return fail(line, format!("{} has no '-> <result>'", what));
    }
    if a + 2 == n && is_int(w[a + 1]) {
        return Ok(());
    }
    if trap_ok
        && a + 3 == n
        && w[a + 1] == "TRAP"
        && is_int(w[a + 2])
        && !w[a + 2].starts_with('-')
        && (1..=14).contains(&c_atoi(w[a + 2]))
    {
        return Ok(());
    }
    if trap_ok {
        fail(
            line,
            format!("{} result must be a decimal number or TRAP <1..14>", what),
        )
    } else {
        fail(line, format!("{} result must be a decimal number", what))
    }
}

fn textline(v: &str, line: usize, what: &str) -> R<()> {
    if v.is_empty() || v.len() > 160 {
        return fail(line, format!("{} must be 1..160 bytes", what));
    }
    Ok(())
}

fn safe_path(p: &str) -> bool {
    let n = p.len();
    if !(1..=255).contains(&n) || p.starts_with('/') {
        return false;
    }
    if !p
        .bytes()
        .all(|c| c.is_ascii_alphanumeric() || b"_./-".contains(&c))
    {
        return false;
    }
    p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

/// family line: gs1-<16 hex> ids, single spaces, strictly ascending
fn famlist(v: &str) -> bool {
    let mut prev: Option<&str> = None;
    if v.is_empty() {
        return false;
    }
    for cur in v.split(' ') {
        let b = cur.as_bytes();
        if b.len() != 20 || !cur.starts_with("gs1-") {
            return false;
        }
        if !b[4..]
            .iter()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
        {
            return false;
        }
        if let Some(p) = prev {
            if p >= cur {
                return false;
            }
        }
        prev = Some(cur);
    }
    true
}

/// recheck `load`: the byte rules of schema section 1; returns the lines.
fn load(bytes: &[u8]) -> R<Vec<&str>> {
    if bytes.len() > MAXB {
        return fail(0, "file over 65536 bytes".into());
    }
    if bytes.is_empty() || bytes[bytes.len() - 1] != b'\n' {
        return fail(0, "file is empty or does not end with LF".into());
    }
    let mut lines = Vec::new();
    let mut line = 1;
    let mut start = 0;
    for (k, &c) in bytes.iter().enumerate() {
        if c == b'\n' {
            // every byte before k on this line is 0x20..0x7E, so this is valid UTF-8
            let s = std::str::from_utf8(&bytes[start..k]).unwrap_or("");
            if s.is_empty() {
                return fail(line, "empty line".into());
            }
            if s.len() > MAXL {
                return fail(line, "line over 1024 bytes".into());
            }
            if s.starts_with(' ') || s.ends_with(' ') {
                return fail(line, "line starts or ends with a space".into());
            }
            if s.contains("  ") {
                return fail(line, "two spaces in a row".into());
            }
            if lines.len() == MAXLINES {
                return fail(line, "too many lines".into());
            }
            lines.push(s);
            start = k + 1;
            line += 1;
        } else if !(0x20..=0x7e).contains(&c) {
            return fail(
                line,
                "byte outside ASCII 0x20..0x7E (CR, tab or non-ASCII)".into(),
            );
        }
    }
    Ok(lines)
}

fn key<'a>(ln: &[&'a str], i: usize, k: &str) -> Option<&'a str> {
    let l = ln.get(i)?;
    let rest = l.strip_prefix(k)?;
    rest.strip_prefix(' ')
}

/// fgets(buf, cap) on `b` from `pos`: up to cap-1 bytes, stopping after a newline.
fn fgets(b: &[u8], pos: usize, cap: usize) -> Option<(&[u8], usize)> {
    if pos >= b.len() {
        return None;
    }
    let lim = (pos + cap - 1).min(b.len());
    let end = b[pos..lim]
        .iter()
        .position(|&c| c == b'\n')
        .map(|p| pos + p + 1)
        .unwrap_or(lim);
    Some((&b[pos..end], end))
}

/// Validates one record like `recheck [--root <root>] [--receipts <dir>] <record>`.
/// `receipts` = None skips the receipt check (no `--receipts`).
pub fn validate(
    bytes: &[u8],
    source_root: Option<&Path>,
    receipts: Option<&dyn ReceiptSource>,
) -> Result<RecordMeta, Refusal> {
    let ln = load(bytes)?;
    let nl = ln.len();
    if nl < 1 || ln[0] != "gen-record 1" {
        return fail(1, "line 1 must be 'gen-record 1'".into());
    }
    let mut i = 1;
    let id_full = match key(&ln, i, "id") {
        Some(v) => v,
        None => return fail(i + 1, "expected 'id <ID>'".into()),
    };
    // recheck keeps the id in a 128-byte buffer
    let id: &str = if id_full.len() > 127 {
        &id_full[..127]
    } else {
        id_full
    };
    i += 1;
    let kind = match key(&ln, i, "kind") {
        Some(v @ ("raw" | "abstraction")) => v,
        _ => return fail(i + 1, "expected 'kind raw' or 'kind abstraction'".into()),
    };
    let abs = kind == "abstraction";
    i += 1;
    if abs {
        let ok = id.len() == 20
            && id.starts_with("abs-")
            && id.as_bytes()[4..]
                .iter()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c));
        if !ok {
            return fail(2, "abstraction id must be abs-<16 hex>".into());
        }
    } else if !(id.starts_with("raw-") && is_taskid(&id[4..])) {
        return fail(2, "raw id must be raw-<task_id>".into());
    }
    let split = match key(&ln, i, "split") {
        Some(v @ ("corpus" | "probe")) => v,
        _ => return fail(i + 1, "expected 'split corpus' or 'split probe'".into()),
    };
    if abs && split != "corpus" {
        return fail(i + 1, "an abstraction record is split corpus".into());
    }
    i += 1;
    let family = match key(&ln, i, "family") {
        Some(v) if famlist(v) => v,
        _ => {
            return fail(
                i + 1,
                "expected 'family <gs1-16 hex>+' (strictly ascending)".into(),
            )
        }
    };
    if !abs && family.contains(' ') {
        return fail(i + 1, "a raw record has exactly one family".into());
    }
    i += 1;
    let entl = i + 1;
    let ent = match key(&ln, i, "entry") {
        Some(v) => v,
        None => return fail(i + 1, "expected 'entry <name>'".into()),
    };
    i += 1;
    let pars = if ln.get(i) == Some(&"params") {
        ""
    } else {
        match key(&ln, i, "params") {
            Some(v) => v,
            None => {
                return fail(
                    i + 1,
                    "expected 'params <type>...' (or just 'params')".into(),
                )
            }
        }
    };
    i += 1;
    let rt = match key(&ln, i, "ret") {
        Some(v) => v,
        None => return fail(i + 1, "expected 'ret <type>'".into()),
    };
    i += 1;
    let sigline = match key(&ln, i, "signature") {
        Some(v) => v,
        None => return fail(i + 1, "expected 'signature <fn line>'".into()),
    };
    let sigl = i + 1;
    let sig = parse_sig(sigline, i + 1)?;
    if ent != sig.name {
        return fail(
            entl,
            "entry differs from the signature's function name".into(),
        );
    }
    {
        let want: Vec<String> = sig
            .params
            .iter()
            .map(|p| {
                if p.len != 0 {
                    let mut it = p.ty.split(' ');
                    let a = it.next().unwrap_or("");
                    let s = it.next().unwrap_or("");
                    format!("{} {} {}", a, irname(s), p.len)
                } else {
                    irname(&p.ty).to_string()
                }
            })
            .collect();
        if pars != want.join(" ") {
            return fail(
                entl + 1,
                "params differ from the signature's parameter types (requirement spelling)".into(),
            );
        }
        let want_ret = if sig.ret == "void" {
            "void"
        } else {
            irname(&sig.ret)
        };
        if rt != want_ret {
            return fail(
                entl + 2,
                "ret differs from the signature's return type (requirement spelling)".into(),
            );
        }
    }
    i += 1;
    match key(&ln, i, "desc") {
        Some(v) => textline(v, i + 1, "desc")?,
        None => return fail(i + 1, "expected 'desc <text>'".into()),
    }
    i += 1;
    let receipt = match key(&ln, i, "receipt") {
        Some(v) if is_hex64(v) => v,
        _ => return fail(i + 1, "expected 'receipt <64 lowercase hex>'".into()),
    };
    i += 1;
    let validity = match key(&ln, i, "validity") {
        Some(v @ ("verified" | "unverified")) => v,
        _ => {
            return fail(
                i + 1,
                "expected 'validity verified' or 'validity unverified'".into(),
            )
        }
    };
    i += 1;
    // ancestry +
    let mut ancestry: Vec<(String, String)> = Vec::new();
    while let Some(v) = key(&ln, i, "ancestry") {
        let (t, d) = match v.split_once(' ') {
            Some((t, d)) if is_taskid(t) && is_hex64(d) => (t, d),
            _ => return fail(i + 1, "expected 'ancestry <task_id> <64 hex>'".into()),
        };
        if let Some((prev, _)) = ancestry.last() {
            if prev.as_str() >= t {
                return fail(
                    i + 1,
                    "ancestry not strictly ascending by task id (or repeated)".into(),
                );
            }
        }
        if !abs && t != &id[4..] {
            return fail(i + 1, "raw record ancestry must be its own task".into());
        }
        ancestry.push((t.to_string(), d.to_string()));
        i += 1;
    }
    let ndist = {
        let mut d: Vec<&str> = ancestry.iter().map(|(_, h)| h.as_str()).collect();
        d.sort_unstable();
        d.dedup();
        d.len()
    };
    if ancestry.is_empty() {
        return fail(i + 1, "expected at least one 'ancestry' line".into());
    }
    if !abs && ancestry.len() != 1 {
        return fail(i + 1, "raw record has exactly one ancestry line".into());
    }
    if abs && (ancestry.len() < 2 || ndist < 2) {
        return fail(
            i + 1,
            "abstraction needs ancestry in >= 2 distinct corpus solutions (task ids and digests)"
                .into(),
        );
    }
    // source +
    let mut sources: Vec<(String, String)> = Vec::new();
    while let Some(v) = key(&ln, i, "source") {
        let (p, d) = match v.rsplit_once(' ') {
            Some((p, d)) if p.len() < 256 => (p, d),
            _ => return fail(i + 1, "expected 'source <path> <64 hex>'".into()),
        };
        if !safe_path(p) || !is_hex64(d) {
            return fail(i + 1, "expected 'source <relative path> <64 hex>'".into());
        }
        if let Some((prev, _)) = sources.last() {
            if prev.as_str() >= p {
                return fail(
                    i + 1,
                    "source not strictly ascending by path (or repeated)".into(),
                );
            }
        }
        if let Some(root) = source_root {
            match std::fs::read(root.join(p)) {
                Err(_) => return fail(i + 1, format!("source file {} cannot be read", p)),
                Ok(b) => {
                    if sha256_hex(&b) != d {
                        return fail(i + 1, format!("stale source digest for {}", p));
                    }
                }
            }
        }
        sources.push((p.to_string(), d.to_string()));
        i += 1;
    }
    if sources.is_empty() {
        return fail(i + 1, "expected at least one 'source' line".into());
    }
    if abs {
        let mut nargs = 0usize;
        for p in &sig.params {
            if p.name == "res" {
                return fail(
                    sigl,
                    "parameter name res is reserved for the postcondition".into(),
                );
            }
            nargs += if p.len != 0 { p.len as usize } else { 1 };
        }
        if !is_scalar(&sig.ret) {
            return fail(sigl, "v1 abstraction helpers return a scalar".into());
        }
        for (k, what) in [("pre", "pre"), ("post", "post")] {
            let v = match key(&ln, i, k) {
                Some(v) => v,
                None => return fail(i + 1, format!("expected '{} STATED+TESTED <value>'", what)),
            };
            if !v.starts_with("STATED+TESTED ") || v.len() == 14 {
                return fail(
                    i + 1,
                    format!(
                        "{} must be tagged STATED+TESTED (v1 proves nothing; PROVED is reserved)",
                        what
                    ),
                );
            }
            i += 1;
        }
        match key(&ln, i, "example") {
            Some(v) => parse_call(v, nargs, false, i + 1, "example")?,
            None => return fail(i + 1, "expected 'example <args> -> <value>'".into()),
        }
        i += 1;
        let mut nlim = 0;
        while let Some(v) = key(&ln, i, "limit") {
            textline(v, i + 1, "limit")?;
            nlim += 1;
            i += 1;
        }
        if nlim == 0 {
            return fail(i + 1, "expected at least one 'limit <text>'".into());
        }
        while let Some(v) = key(&ln, i, "counter") {
            parse_call(v, nargs, true, i + 1, "counter")?;
            i += 1;
        }
        let fpr = match key(&ln, i, "fingerprint") {
            Some(v) if is_hex64(v) => v,
            _ => return fail(i + 1, "expected 'fingerprint <64 hex>'".into()),
        };
        if fpr[..16] != id[4..20] {
            return fail(
                i + 1,
                "id is not abs- + the first 16 hex of the fingerprint".into(),
            );
        }
        i += 1;
        let mut prevd: Option<&str> = None;
        let mut nd = 0;
        while let Some(v) = key(&ln, i, "differs") {
            let (t, d) = match v.split_once(' ') {
                Some((t, d)) if is_taskid(t) && is_hex64(d) => (t, d),
                _ => return fail(i + 1, "expected 'differs <task_id> <64 hex>'".into()),
            };
            if let Some(p) = prevd {
                if p >= t {
                    return fail(
                        i + 1,
                        "differs not strictly ascending by task id (or repeated)".into(),
                    );
                }
            }
            if d == fpr {
                return fail(
                    i + 1,
                    format!(
                        "helper fingerprint equals the whole corpus solution of {}",
                        t
                    ),
                );
            }
            prevd = Some(t);
            nd += 1;
            i += 1;
        }
        if nd < 2 {
            return fail(
                i + 1,
                "expected a 'differs' line for every corpus solution (at least 2)".into(),
            );
        }
    }
    // body
    if i >= nl || ln[i] != "body" {
        return fail(i + 1, "expected 'body'".into());
    }
    let b0 = i + 1;
    if nl < b0 + 1 || nl - b0 > 400 {
        return fail(i + 1, "body must have 1..400 lines".into());
    }
    let fns: Vec<usize> = (b0..nl).filter(|&k| ln[k].starts_with("fn ")).collect();
    if fns.is_empty() {
        return fail(b0 + 1, "body has no 'fn' line".into());
    }
    if ln[nl - 1] != "end" {
        return fail(nl, "body must end with 'end'".into());
    }
    if abs {
        if fns.len() != 1 || !ln[b0].starts_with("fn ") {
            return fail(
                b0 + 1,
                "abstraction body is exactly one function starting at its first line".into(),
            );
        }
        if ln[b0] != sigline {
            return fail(sigl, "signature differs from the body's fn line".into());
        }
    } else if ln[*fns.last().unwrap_or(&b0)] != sigline {
        return fail(
            sigl,
            "signature differs from the body's last fn line (the entry)".into(),
        );
    }
    // receipt file
    if let Some(src) = receipts {
        let rb = match src.receipt(receipt) {
            Some(b) => b,
            None => return fail(11, format!("receipt file {} is missing", receipt)),
        };
        if sha256_hex(&rb) != receipt {
            return fail(
                11,
                "receipt file does not hash to its id (fake or edited receipt)".into(),
            );
        }
        let l1 = fgets(&rb, 0, 64);
        let l2 = l1.and_then(|(_, p)| fgets(&rb, p, 160));
        let (l1, l2) = match (l1, l2) {
            (Some((a, _)), Some((b, _))) => (a, b),
            _ => return fail(11, "receipt file is too short".into()),
        };
        let want = format!("record {}\n", id);
        if l1 != b"gen-receipt 1\n" || l2 != want.as_bytes() {
            return fail(
                11,
                "receipt is not this record's (header or record line)".into(),
            );
        }
    }
    Ok(RecordMeta {
        id: id_full.to_string(),
        kind: kind.to_string(),
        split: split.to_string(),
        families: family.split(' ').map(str::to_string).collect(),
        validity: validity.to_string(),
        receipt: receipt.to_string(),
        ancestry,
        sources,
    })
}
