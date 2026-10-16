//! The item oracle: what may come next after a prefix of an Omegese program.
//!
//! The contract is the fixed grammar of `omegese next --machine` (omega
//! `docs/one_layer/OMEGESE.md` section 6.2): one item per line, exit 0 with at least one item, or
//! exit 1 with exactly one line starting `REFUSED omegese: `. [`ItemOracle`] is that contract as
//! a trait; [`SubprocessOracle`] runs the real binary once per query.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One line of `omegese next --machine`. Let U be the word still being written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Item {
    /// `word X`: finish the current word as X (X starts with U).
    Word(Vec<u8>),
    /// `newname E1 E2 ...`: finish the current word as any new name that starts with U, is not a
    /// reserved word and is not one of `exclude`.
    NewName { exclude: Vec<Vec<u8>> },
    /// `number LO HI`: finish the current word as a number in `lo..=hi` whose text starts with U.
    Number { lo: i128, hi: i128 },
    /// `(`: an opening bracket (it also ends the current word).
    Open,
    /// `)`: a closing bracket (it also ends the current word).
    Close,
    /// `space`: a space or tab.
    Space,
    /// `newline`: ends the current word and the line.
    Newline,
    /// `eof`: the prefix is already an accepted program.
    Eof,
}

/// The oracle's answer for one prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Exit 0: at least one item.
    Items(Vec<Item>),
    /// Exit 1: no text after this prefix makes a program `omegese emit` accepts. The whole line.
    Refused(String),
}

/// What may come next after a prefix (section 6.2 is the contract).
pub trait ItemOracle {
    /// The answer for `prefix`. `Err` only for a broken oracle (I/O, a line outside the grammar);
    /// a prefix with no legal continuation is `Ok(Answer::Refused)`.
    fn next(&mut self, prefix: &[u8]) -> Result<Answer, String>;
}

impl<O: ItemOracle + ?Sized> ItemOracle for Box<O> {
    fn next(&mut self, prefix: &[u8]) -> Result<Answer, String> {
        (**self).next(prefix)
    }
}

/// Parses one `omegese next --machine` run: its stdout and whether it exited 0 (`true`) or 1.
pub fn parse_machine(stdout: &str, exit_zero: bool) -> Result<Answer, String> {
    let lines: Vec<&str> = stdout.lines().collect();
    if !exit_zero {
        return match lines.as_slice() {
            [l] if l.starts_with("REFUSED omegese: ") => Ok(Answer::Refused(l.to_string())),
            _ => Err(format!(
                "oracle exit 1 must print exactly one REFUSED line, got {} lines",
                lines.len()
            )),
        };
    }
    if lines.is_empty() {
        return Err("oracle exit 0 with no item".into());
    }
    lines
        .iter()
        .map(|l| parse_item(l))
        .collect::<Result<Vec<_>, _>>()
        .map(Answer::Items)
}

fn parse_item(line: &str) -> Result<Item, String> {
    let bad = || format!("oracle line outside the 6.2 grammar: {line:?}");
    let mut parts = line.split(' ');
    let head = parts.next().ok_or_else(bad)?;
    let rest: Vec<&str> = parts.collect();
    let item = match (head, rest.as_slice()) {
        ("word", [x]) if !x.is_empty() => Item::Word(x.as_bytes().to_vec()),
        ("newname", names) => Item::NewName {
            exclude: names
                .iter()
                .map(|n| {
                    if n.is_empty() {
                        Err(bad())
                    } else {
                        Ok(n.as_bytes().to_vec())
                    }
                })
                .collect::<Result<_, _>>()?,
        },
        ("number", [lo, hi]) => {
            let lo: i128 = lo.parse().map_err(|_| bad())?;
            let hi: i128 = hi.parse().map_err(|_| bad())?;
            if lo > hi {
                return Err(bad());
            }
            Item::Number { lo, hi }
        }
        ("(", []) => Item::Open,
        (")", []) => Item::Close,
        ("space", []) => Item::Space,
        ("newline", []) => Item::Newline,
        ("eof", []) => Item::Eof,
        _ => return Err(bad()),
    };
    Ok(item)
}

/// The real oracle: `<bin> next --machine /dev/stdin`, the prefix on stdin, one process per query.
/// The binary's path is configuration (built from omega `tools/omegese/omegese.c`).
pub struct SubprocessOracle {
    bin: PathBuf,
    calls: u64,
    elapsed: Duration,
}

impl SubprocessOracle {
    /// Checks that `bin` is a file; the first query proves it runs.
    pub fn new(bin: impl Into<PathBuf>) -> Result<Self, String> {
        let bin = bin.into();
        if !bin.is_file() {
            return Err(format!("omegese binary not found at {}", bin.display()));
        }
        Ok(Self {
            bin,
            calls: 0,
            elapsed: Duration::ZERO,
        })
    }

    /// Queries made so far.
    pub fn calls(&self) -> u64 {
        self.calls
    }

    /// Wall time spent in queries so far (spawn, write, wait, parse).
    pub fn elapsed(&self) -> Duration {
        self.elapsed
    }
}

impl ItemOracle for SubprocessOracle {
    fn next(&mut self, prefix: &[u8]) -> Result<Answer, String> {
        let t = Instant::now();
        let mut child = Command::new(&self.bin)
            .args(["next", "--machine", "/dev/stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("spawn {}: {e}", self.bin.display()))?;
        let mut stdin = child.stdin.take().ok_or("oracle stdin missing")?;
        // A writer thread, so a prefix larger than the pipe buffer cannot deadlock against stdout.
        let out = std::thread::scope(|s| {
            let w = s.spawn(move || stdin.write_all(prefix));
            let out = child.wait_with_output();
            let wrote = w
                .join()
                .map_err(|_| "oracle writer thread panicked".to_string())?;
            wrote.map_err(|e| format!("write the prefix to the oracle: {e}"))?;
            out.map_err(|e| format!("wait for the oracle: {e}"))
        })?;
        self.calls += 1;
        let stdout = String::from_utf8(out.stdout).map_err(|_| "oracle stdout is not UTF-8")?;
        let r = match out.status.code() {
            Some(0) => parse_machine(&stdout, true),
            Some(1) => parse_machine(&stdout, false),
            c => Err(format!(
                "oracle exit {c:?}: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )),
        };
        self.elapsed += t.elapsed();
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_section_6_2_example_parses() {
        let out = "word if\nword while\nword for\nword ret\nword trap\nword a\nnewname a f\nspace\nnewline\n";
        let Answer::Items(items) = parse_machine(out, true).unwrap() else {
            panic!("items")
        };
        assert_eq!(items.len(), 9);
        assert_eq!(items[0], Item::Word(b"if".to_vec()));
        assert_eq!(
            items[6],
            Item::NewName {
                exclude: vec![b"a".to_vec(), b"f".to_vec()]
            }
        );
        assert_eq!(items[8], Item::Newline);
    }

    #[test]
    fn numbers_span_u64_and_i64() {
        let out =
            "number 0 18446744073709551615\nnumber -9223372036854775808 -1\n(\n)\neof\nnewname\n";
        let Answer::Items(items) = parse_machine(out, true).unwrap() else {
            panic!("items")
        };
        assert_eq!(
            items[0],
            Item::Number {
                lo: 0,
                hi: u64::MAX as i128
            }
        );
        assert_eq!(
            items[1],
            Item::Number {
                lo: i64::MIN as i128,
                hi: -1
            }
        );
        assert_eq!(&items[2..5], &[Item::Open, Item::Close, Item::Eof]);
        assert_eq!(items[5], Item::NewName { exclude: vec![] });
    }

    #[test]
    fn refusals_and_contract_breaks() {
        let r = "REFUSED omegese: line 2: no text after this prefix makes a program emit accepts\n";
        assert!(matches!(parse_machine(r, false), Ok(Answer::Refused(_))));
        assert!(parse_machine("", true).is_err(), "exit 0 needs an item");
        assert!(parse_machine("", false).is_err(), "exit 1 needs its line");
        assert!(parse_machine("word if\n", false).is_err());
        assert!(parse_machine("word\n", true).is_err());
        assert!(parse_machine("word a b\n", true).is_err());
        assert!(parse_machine("number 5 1\n", true).is_err());
        assert!(parse_machine("number x 1\n", true).is_err());
        assert!(parse_machine("space \n", true).is_err());
        assert!(parse_machine("tab\n", true).is_err());
    }
}
