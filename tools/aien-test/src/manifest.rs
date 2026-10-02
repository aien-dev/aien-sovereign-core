//! Gate manifest v1 parser (ADR 0028 Decision 3). Strict: unknown keys,
//! duplicate keys, tabs, CRLF, BOM, deep indentation and trailing garbage are
//! all errors.
//!
//! Slice A deviation (recorded): the ADR's full example nests the `observe`
//! list one level deeper than the normative grammar allows. The grammar wins:
//! `observe` is a one-line JSON list, for example
//! `  observe: ["sqrt_max_ulp <= 1", "chip_verdict == \"PASS\""]`.
//! The `adapter` key is not part of Slice A and is rejected as unknown.

use crate::evidence::sha256_hex;
use serde_json::Value;
use std::fmt;

pub const MAX_LINES: usize = 200;
pub const MAX_BYTES: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestError(pub String);

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn err<T>(line: usize, msg: &str) -> Result<T, ManifestError> {
    Err(ManifestError(format!("line {line}: {msg}")))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quant {
    Last,
    All,
    Any,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    pub text: String,
    pub quant: Quant,
    pub name: String,
    pub op: Op,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    pub target: String,
    pub tool: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub gate: String,
    pub owner: String,
    pub requires: Vec<String>,
    pub depends_on: Vec<String>,
    pub inputs: Vec<String>,
    pub build: Option<Build>,
    pub exec: String,
    pub args: Vec<String>,
    pub expect_exit: i64,
    pub rules: Vec<Rule>,
    pub timeout_ms: u64,
    pub mutants: Vec<String>,
    pub checks: Vec<String>,
    pub cache_allowed: bool,
    /// sha256 of the exact manifest file bytes (P7).
    pub digest: String,
}

impl Manifest {
    /// The scarcest listed pool: gb10 > qemu > host.
    pub fn pool(&self) -> &'static str {
        if self.requires.iter().any(|r| r == "gb10") {
            "gb10"
        } else if self.requires.iter().any(|r| r == "qemu") {
            "qemu"
        } else {
            "host"
        }
    }
}

#[derive(Debug, Clone)]
enum Val {
    Str(String),
    List(Vec<String>),
}

struct Entry {
    key: String,
    line: usize,
    scalar: Option<Val>,
    items: Vec<String>,
    children: Vec<(String, Val, usize)>,
}

pub fn valid_key(k: &str) -> bool {
    let mut chars = k.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub fn valid_gate_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
}

/// Observation NAME: [a-z][a-z0-9_.-]*
pub fn valid_obs_name(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.' || c == '-')
}

fn valid_rel_path(s: &str) -> bool {
    !s.is_empty() && !s.starts_with('/') && !s.contains('\0') && !s.split('/').any(|c| c == "..")
}

fn parse_val(s: &str, line: usize) -> Result<Val, ManifestError> {
    if s.starts_with('"') {
        let mut it = serde_json::Deserializer::from_str(s).into_iter::<String>();
        let v = match it.next() {
            Some(Ok(v)) => v,
            _ => return err(line, "bad quoted string"),
        };
        if !s[it.byte_offset()..].is_empty() {
            return err(line, "trailing garbage after quoted string");
        }
        return Ok(Val::Str(v));
    }
    if s.starts_with('[') {
        return match serde_json::from_str::<Vec<String>>(s) {
            Ok(v) => Ok(Val::List(v)),
            Err(_) => err(line, "list must be a one-line JSON array of strings"),
        };
    }
    let cut = match s.find(" #") {
        Some(i) => &s[..i],
        None => s,
    };
    let t = cut.trim_end();
    if t.is_empty() {
        return err(line, "empty value");
    }
    Ok(Val::Str(t.to_string()))
}

/// Split `KEY: VALUE` (value optional).
fn split_key(s: &str, line: usize) -> Result<(String, Option<Val>), ManifestError> {
    let i = match s.find(':') {
        Some(i) => i,
        None => return err(line, "expected KEY: or an item line"),
    };
    let key = &s[..i];
    if !valid_key(key) {
        return err(line, "invalid key");
    }
    let rest = &s[i + 1..];
    if rest.is_empty() {
        return Ok((key.to_string(), None));
    }
    if !rest.starts_with(' ') {
        return err(line, "space required after colon");
    }
    let v = &rest[1..];
    if v.starts_with(' ') {
        return err(line, "extra space after colon");
    }
    Ok((key.to_string(), Some(parse_val(v, line)?)))
}

fn parse_entries(text: &str) -> Result<Vec<Entry>, ManifestError> {
    let mut entries: Vec<Entry> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let raw = raw.trim_end_matches(' ');
        let trimmed = raw.trim_start_matches(' ');
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = raw.strip_prefix("  - ") {
            if rest.starts_with(' ') {
                return err(n, "extra space after dash");
            }
            let v = match parse_val(rest, n)? {
                Val::Str(s) => s,
                Val::List(_) => return err(n, "list item must be a scalar"),
            };
            match entries.last_mut() {
                Some(e) => e.items.push(v),
                None => return err(n, "item line before any key"),
            }
        } else if let Some(rest) = raw.strip_prefix("  ") {
            if rest.starts_with(' ') {
                return err(n, "indentation deeper than two spaces");
            }
            let (k, v) = split_key(rest, n)?;
            let v = match v {
                Some(v) => v,
                None => return err(n, "child key needs a value"),
            };
            match entries.last_mut() {
                Some(e) => {
                    if e.children.iter().any(|c| c.0 == k) {
                        return err(n, "duplicate child key");
                    }
                    e.children.push((k, v, n));
                }
                None => return err(n, "child line before any key"),
            }
        } else if raw.starts_with(' ') {
            return err(n, "one-space indentation");
        } else {
            let (k, v) = split_key(raw, n)?;
            if entries.iter().any(|e| e.key == k) {
                return err(n, &format!("duplicate key {k}"));
            }
            entries.push(Entry {
                key: k,
                line: n,
                scalar: v,
                items: Vec::new(),
                children: Vec::new(),
            });
        }
    }
    for e in &entries {
        let shapes = (e.scalar.is_some() as u8)
            + (!e.items.is_empty() as u8)
            + (!e.children.is_empty() as u8);
        if shapes > 1 {
            return err(e.line, &format!("key {} mixes value shapes", e.key));
        }
    }
    Ok(entries)
}

fn scalar(e: &Entry) -> Result<String, ManifestError> {
    match (&e.scalar, e.items.is_empty(), e.children.is_empty()) {
        (Some(Val::Str(s)), true, true) => Ok(s.clone()),
        _ => err(e.line, &format!("key {} needs a scalar value", e.key)),
    }
}

fn list(e: &Entry) -> Result<Vec<String>, ManifestError> {
    match (&e.scalar, e.children.is_empty()) {
        (Some(Val::List(l)), true) if e.items.is_empty() => Ok(l.clone()),
        (None, true) => Ok(e.items.clone()),
        _ => err(e.line, &format!("key {} needs a list", e.key)),
    }
}

fn map<'a>(e: &'a Entry, allowed: &[&str]) -> Result<&'a Vec<(String, Val, usize)>, ManifestError> {
    if e.scalar.is_some() || !e.items.is_empty() || e.children.is_empty() {
        return err(e.line, &format!("key {} needs child keys", e.key));
    }
    for (k, _, n) in &e.children {
        if !allowed.contains(&k.as_str()) {
            return err(*n, &format!("unknown child key {k} under {}", e.key));
        }
    }
    Ok(&e.children)
}

fn child<'a>(c: &'a [(String, Val, usize)], k: &str) -> Option<&'a (String, Val, usize)> {
    c.iter().find(|x| x.0 == k)
}

fn child_str(c: &[(String, Val, usize)], k: &str) -> Result<Option<String>, ManifestError> {
    match child(c, k) {
        None => Ok(None),
        Some((_, Val::Str(s), _)) => Ok(Some(s.clone())),
        Some((_, Val::List(_), n)) => err(*n, &format!("{k} must be a scalar")),
    }
}

pub fn parse_duration_ms(s: &str) -> Option<u64> {
    let (num, mult) = if let Some(n) = s.strip_suffix("ms") {
        (n, 1u64)
    } else if let Some(n) = s.strip_suffix('s') {
        (n, 1000)
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 60_000)
    } else if let Some(n) = s.strip_suffix('h') {
        (n, 3_600_000)
    } else {
        return None;
    };
    if num.is_empty() || !num.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let v: u64 = num.parse().ok()?;
    if v == 0 {
        return None;
    }
    v.checked_mul(mult)
}

pub fn parse_rule(text: &str) -> Result<Rule, String> {
    let (quant, rest) = if let Some(r) = text.strip_prefix("all ") {
        (Quant::All, r)
    } else if let Some(r) = text.strip_prefix("any ") {
        (Quant::Any, r)
    } else {
        (Quant::Last, text)
    };
    let mut it = rest.splitn(3, ' ');
    let name = it.next().unwrap_or("");
    let op = it.next().unwrap_or("");
    let val = it.next().unwrap_or("");
    if !valid_obs_name(name) {
        return Err(format!("bad observation name in rule {text:?}"));
    }
    let op = match op {
        "==" => Op::Eq,
        "!=" => Op::Ne,
        "<" => Op::Lt,
        "<=" => Op::Le,
        ">" => Op::Gt,
        ">=" => Op::Ge,
        _ => return Err(format!("bad operator in rule {text:?}")),
    };
    let value =
        parse_json_scalar(val.trim()).ok_or_else(|| format!("bad value in rule {text:?}"))?;
    Ok(Rule {
        text: text.to_string(),
        quant,
        name: name.to_string(),
        op,
        value,
    })
}

/// JSON string, integer (no floats), true/false/null.
pub fn parse_json_scalar(s: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(s).ok()?;
    let ok = match &v {
        Value::String(_) | Value::Bool(_) | Value::Null => true,
        Value::Number(n) => n.is_i64(),
        _ => false,
    };
    if ok {
        Some(v)
    } else {
        None
    }
}

pub fn parse(bytes: &[u8]) -> Result<Manifest, ManifestError> {
    if bytes.len() > MAX_BYTES {
        return Err(ManifestError("manifest larger than 16 KiB".into()));
    }
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(ManifestError("BOM not allowed".into()));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| ManifestError("manifest is not valid UTF-8".into()))?;
    if text.contains('\r') {
        return Err(ManifestError(
            "CR not allowed (Unix line endings only)".into(),
        ));
    }
    if text.contains('\t') {
        return Err(ManifestError("tab not allowed".into()));
    }
    if text.lines().count() > MAX_LINES {
        return Err(ManifestError("manifest longer than 200 lines".into()));
    }
    let entries = parse_entries(text)?;

    const KNOWN: [&str; 13] = [
        "gate",
        "manifest_version",
        "owner",
        "requires",
        "depends_on",
        "inputs",
        "build",
        "run",
        "expects",
        "timeout",
        "mutants",
        "checks",
        "cache",
    ];
    for e in &entries {
        if !KNOWN.contains(&e.key.as_str()) {
            return err(e.line, &format!("unknown key {}", e.key));
        }
    }
    let get = |k: &str| entries.iter().find(|e| e.key == k);
    let need = |k: &str| match get(k) {
        Some(e) => Ok(e),
        None => Err(ManifestError(format!("missing required key {k}"))),
    };

    let gate = scalar(need("gate")?)?;
    if !valid_gate_id(&gate) {
        return Err(ManifestError(format!("invalid gate id {gate:?}")));
    }
    if scalar(need("manifest_version")?)? != "1" {
        return Err(ManifestError(
            "unsupported manifest_version (only 1)".into(),
        ));
    }
    let owner = scalar(need("owner")?)?;

    let requires = list(need("requires")?)?;
    for r in &requires {
        if !["host", "qemu", "gb10", "operator"].contains(&r.as_str()) {
            return Err(ManifestError(format!("unknown requires entry {r:?}")));
        }
    }
    if !requires
        .iter()
        .any(|r| r == "host" || r == "qemu" || r == "gb10")
    {
        return Err(ManifestError(
            "requires must list one of host, qemu, gb10".into(),
        ));
    }

    let depends_on = match get("depends_on") {
        Some(e) => list(e)?,
        None => Vec::new(),
    };
    for d in &depends_on {
        if !valid_gate_id(d) {
            return Err(ManifestError(format!("invalid depends_on id {d:?}")));
        }
    }
    let inputs = match get("inputs") {
        Some(e) => list(e)?,
        None => Vec::new(),
    };
    for i in &inputs {
        if !valid_rel_path(i) {
            return Err(ManifestError(format!(
                "inputs path must be repo-relative: {i:?}"
            )));
        }
    }

    let build = match get("build") {
        None => None,
        Some(e) => {
            let c = map(e, &["target", "tool"])?;
            let tool =
                child_str(c, "tool")?.ok_or_else(|| ManifestError("build.tool missing".into()))?;
            let target = child_str(c, "target")?
                .ok_or_else(|| ManifestError("build.target missing".into()))?;
            if !["make", "cargo", "none"].contains(&tool.as_str()) {
                return Err(ManifestError(format!("unknown build.tool {tool:?}")));
            }
            Some(Build { target, tool })
        }
    };

    let (exec, args) = {
        let c = map(need("run")?, &["exec", "args"])?;
        let exec = child_str(c, "exec")?.ok_or_else(|| ManifestError("run.exec missing".into()))?;
        if !valid_rel_path(&exec) {
            return Err(ManifestError(
                "run.exec must be a repo-relative path".into(),
            ));
        }
        let args = match child(c, "args") {
            None => Vec::new(),
            Some((_, Val::List(l), _)) => l.clone(),
            Some((_, Val::Str(_), n)) => return err(*n, "run.args must be a one-line list"),
        };
        (exec, args)
    };

    let (expect_exit, rules) = {
        let c = map(need("expects")?, &["exit", "observe", "verdict"])?;
        match child_str(c, "verdict")? {
            Some(v) if v == "PASS" => {}
            _ => return Err(ManifestError("expects.verdict must be PASS in v1".into())),
        }
        let exit = match child_str(c, "exit")? {
            None => 0,
            Some(s) => s
                .parse::<i64>()
                .map_err(|_| ManifestError("expects.exit must be an integer".into()))?,
        };
        let texts: Vec<String> = match child(c, "observe") {
            None => Vec::new(),
            Some((_, Val::List(l), _)) => l.clone(),
            Some((_, Val::Str(s), _)) => vec![s.clone()],
        };
        let mut rules = Vec::new();
        for t in &texts {
            rules.push(parse_rule(t).map_err(ManifestError)?);
        }
        (exit, rules)
    };

    let timeout_s = scalar(need("timeout")?)?;
    let timeout_ms = parse_duration_ms(&timeout_s)
        .ok_or_else(|| ManifestError(format!("bad timeout {timeout_s:?}")))?;

    let mutants = match get("mutants") {
        Some(e) => list(e)?,
        None => Vec::new(),
    };
    for m in &mutants {
        if m.is_empty()
            || !m
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(ManifestError(format!("bad mutant name {m:?}")));
        }
    }
    let checks = match get("checks") {
        Some(e) => list(e)?,
        None => Vec::new(),
    };
    for c in &checks {
        if !["no_libm_symbols", "no_cuda_symbols", "clean_tree_after"].contains(&c.as_str()) {
            return Err(ManifestError(format!("unknown check {c:?}")));
        }
    }
    let cache_allowed = match get("cache") {
        None => true,
        Some(e) => match scalar(e)?.as_str() {
            "allow" => true,
            "never" => false,
            other => {
                return Err(ManifestError(format!(
                    "cache must be allow or never, got {other:?}"
                )))
            }
        },
    };

    Ok(Manifest {
        gate,
        owner,
        requires,
        depends_on,
        inputs,
        build,
        exec,
        args,
        expect_exit,
        rules,
        timeout_ms,
        mutants,
        checks,
        cache_allowed,
        digest: sha256_hex(bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    pub const GOOD: &str = concat!(
        "# comment\n",
        "gate: G-1\n",
        "manifest_version: 1\n",
        "owner: omega\n",
        "requires:\n",
        "  - host\n",
        "run:\n",
        "  exec: tools/run.sh\n",
        "  args: [\"--a\", \"b c\"]\n",
        "expects:\n",
        "  exit: 0\n",
        "  observe: [\"n == 3\", \"all x <= 2\", \"s == \\\"ok\\\"\"]\n",
        "  verdict: PASS\n",
        "timeout: 5s\n",
    );

    fn with(extra: &str) -> String {
        format!("{GOOD}{extra}")
    }

    #[test]
    fn good_manifest_parses() {
        let m = parse(GOOD.as_bytes()).unwrap();
        assert_eq!(m.gate, "G-1");
        assert_eq!(m.owner, "omega");
        assert_eq!(m.pool(), "host");
        assert_eq!(m.exec, "tools/run.sh");
        assert_eq!(m.args, vec!["--a".to_string(), "b c".to_string()]);
        assert_eq!(m.timeout_ms, 5000);
        assert_eq!(m.rules.len(), 3);
        assert_eq!(m.rules[1].quant, Quant::All);
        assert_eq!(m.digest, sha256_hex(GOOD.as_bytes()));
        assert!(m.cache_allowed);
    }

    #[test]
    fn digest_covers_comments() {
        let a = parse(GOOD.as_bytes()).unwrap();
        let b = parse(with("# more\n").as_bytes()).unwrap();
        assert_ne!(a.digest, b.digest);
    }

    #[test]
    fn duplicate_top_level_key_rejected() {
        assert!(parse(with("owner: x\n").as_bytes()).is_err());
    }

    #[test]
    fn duplicate_child_key_rejected() {
        let t = GOOD.replace("  exit: 0\n", "  exit: 0\n  exit: 1\n");
        assert!(parse(t.as_bytes()).is_err());
    }

    #[test]
    fn unknown_top_level_key_rejected() {
        assert!(parse(with("surprise: 1\n").as_bytes()).is_err());
    }

    #[test]
    fn unknown_child_key_rejected() {
        let t = GOOD.replace(
            "  exec: tools/run.sh\n",
            "  exec: tools/run.sh\n  shell: sh\n",
        );
        assert!(parse(t.as_bytes()).is_err());
    }

    #[test]
    fn tab_crlf_bom_rejected() {
        assert!(parse(GOOD.replace("owner: omega", "owner:\tomega").as_bytes()).is_err());
        assert!(parse(GOOD.replace('\n', "\r\n").as_bytes()).is_err());
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice(GOOD.as_bytes());
        assert!(parse(&b).is_err());
        assert!(parse(&[0xFF, 0xFE, b'\n']).is_err());
    }

    #[test]
    fn deep_indent_rejected() {
        let t = GOOD.replace(
            "  exec: tools/run.sh\n",
            "  exec: tools/run.sh\n    deeper: 1\n",
        );
        assert!(parse(t.as_bytes()).is_err());
    }

    #[test]
    fn mixed_shapes_rejected() {
        let t = GOOD.replace("requires:\n  - host\n", "requires: [\"host\"]\n  - host\n");
        assert!(parse(t.as_bytes()).is_err());
    }

    #[test]
    fn trailing_garbage_rejected() {
        assert!(parse(
            GOOD.replace("owner: omega", "owner: \"omega\" x")
                .as_bytes()
        )
        .is_err());
    }

    #[test]
    fn size_limits() {
        let mut big = String::from(GOOD);
        for _ in 0..200 {
            big.push_str("# pad\n");
        }
        assert!(parse(big.as_bytes()).is_err());
        let huge = format!("{GOOD}# {}\n", "x".repeat(MAX_BYTES));
        assert!(parse(huge.as_bytes()).is_err());
    }

    #[test]
    fn missing_required_and_bad_version() {
        assert!(parse(GOOD.replace("timeout: 5s\n", "").as_bytes()).is_err());
        assert!(parse(
            GOOD.replace("manifest_version: 1", "manifest_version: 2")
                .as_bytes()
        )
        .is_err());
        assert!(parse(
            GOOD.replace("  verdict: PASS\n", "  verdict: FAIL\n")
                .as_bytes()
        )
        .is_err());
    }

    #[test]
    fn requires_needs_a_pool() {
        assert!(parse(GOOD.replace("  - host\n", "  - operator\n").as_bytes()).is_err());
        assert!(parse(GOOD.replace("  - host\n", "  - moon\n").as_bytes()).is_err());
    }

    #[test]
    fn pool_is_scarcest() {
        let t = GOOD.replace("  - host\n", "  - host\n  - gb10\n");
        assert_eq!(parse(t.as_bytes()).unwrap().pool(), "gb10");
    }

    #[test]
    fn paths_must_be_relative() {
        assert!(parse(GOOD.replace("tools/run.sh", "/bin/sh").as_bytes()).is_err());
        assert!(parse(GOOD.replace("tools/run.sh", "../x").as_bytes()).is_err());
    }

    #[test]
    fn durations() {
        assert_eq!(parse_duration_ms("250ms"), Some(250));
        assert_eq!(parse_duration_ms("2m"), Some(120_000));
        assert_eq!(parse_duration_ms("1h"), Some(3_600_000));
        assert_eq!(parse_duration_ms("0s"), None);
        assert_eq!(parse_duration_ms("5"), None);
        assert_eq!(parse_duration_ms("s"), None);
    }

    #[test]
    fn rules_parse_and_reject_floats() {
        let r = parse_rule("fence_count == 3").unwrap();
        assert_eq!(r.op, Op::Eq);
        assert_eq!(r.value, Value::from(3));
        assert!(parse_rule("x <= 1.5").is_err());
        assert!(parse_rule("X == 1").is_err());
        assert!(parse_rule("x ~ 1").is_err());
        assert!(parse_rule("x ==").is_err());
        assert_eq!(parse_rule("any v > 2").unwrap().quant, Quant::Any);
        assert_eq!(
            parse_rule("s == \"a b\"").unwrap().value,
            Value::from("a b")
        );
    }

    #[test]
    fn comment_inside_bare_value() {
        let m = parse(
            GOOD.replace("owner: omega", "owner: omega # note")
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(m.owner, "omega");
        let m = parse(GOOD.replace("owner: omega", "owner: a#b").as_bytes()).unwrap();
        assert_eq!(m.owner, "a#b");
    }
}
