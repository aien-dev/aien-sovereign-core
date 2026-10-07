//! Which path does a compose goal ask to WRITE? (issue #288)
//!
//! A goal mentions paths in two roles: the DESTINATION (the file the task
//! creates or changes) and SOURCES (material it reads: "about README.md").
//! Only the destination decides the task kind, the budget, the prompt and the
//! merge. This module is a small rule-based parser with two tables:
//!
//! * `DESTINATION_VERBS`: a verb that introduces the destination ("create",
//!   "update", "append", "save", ...). The first path after it is the target.
//! * `SOURCE_MARKERS`: a word that marks the next path as source material
//!   ("about", "from", "using", "based on", "summarising", ...). Paths joined
//!   to a source by "and" or a comma are sources too.
//!
//! Rules, in order:
//! 1. A path-like word is one with a `/`, a file extension, or a name that
//!    exists as a file (`Makefile`). Absolute, `~` and `..` paths are path-like
//!    and unsafe.
//! 2. Paths marked as sources are set aside.
//! 3. No destination left: `Ok(None)` (a new document, the model names it).
//! 4. One destination: it.
//! 5. Several destinations: the first is taken only when a destination verb
//!    comes before it and no later path is joined to it by "and"/"," or
//!    introduced by a second destination verb. Otherwise `Err(Ambiguous)`
//!    naming every candidate. Never a silent pick.
//!
//! The words of a goal are untrusted text; nothing here touches the disk
//! except through the `is_file` callback.

/// Verbs that introduce the destination of a task.
pub const DESTINATION_VERBS: &[&str] = &[
    "create", "write", "make", "generate", "produce", "draft", "compose", "update", "edit",
    "modify", "append", "add", "insert", "rewrite", "change", "fix", "extend", "revise", "save",
    "put", "store", "place", "prepend", "replace",
];

/// Words that mark the path after them as source material.
pub const SOURCE_MARKERS: &[&str] = &[
    "about",
    "from",
    "using",
    "use",
    "of",
    "reading",
    "read",
    "summarising",
    "summarizing",
    "summarise",
    "summarize",
    "describing",
    "covering",
    "per",
    "following",
    "reference",
    "referencing",
    "given",
];

/// Words that may sit between a destination verb and the path it governs
/// ("Edit the given README.md"): a verb reaching the path through these wins
/// over a source marker.
pub const FILLERS: &[&str] = &[
    "the",
    "a",
    "an",
    "this",
    "that",
    "my",
    "our",
    "your",
    "file",
    "document",
    "doc",
    "new",
    "existing",
    "given",
    "reference",
    "referenced",
    "called",
    "named",
    "same",
    "current",
];

/// Nouns that place a position INSIDE a file: in "the end of X" or "the Intro
/// section of X", the word "of" points at the file being changed, so it is not
/// a source marker. Other "of" phrases ("summary of X", "list of X", "copy of
/// X") still mark X as material to read.
pub const LOCATIONAL: &[&str] = &[
    "section",
    "sections",
    "part",
    "end",
    "top",
    "bottom",
    "start",
    "beginning",
    "body",
    "contents",
    "content",
    "heading",
    "line",
    "lines",
    "row",
    "block",
    "footer",
    "header",
    "middle",
    "rest",
    "bit",
];

/// Top-level domains that make a bare `name.tld` token a host name, not a file.
pub const HOST_TLDS: &[&str] = &[
    "org", "com", "net", "io", "dev", "edu", "gov", "ai", "co", "app", "me", "info", "xyz",
];

/// Words after which a `name.tld` token is a web address.
pub const HOST_LEAD: &[&str] = &["visit", "see", "at", "http", "https", "www", "url", "site"];

/// A path-like word of the goal.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Cand {
    path: String,
    idx: usize,
    source: bool,
    /// Joined to the previous path-like word by "and", "or" or a comma.
    conj: bool,
    /// A destination verb stands between the previous path-like word and this one.
    verb_before: bool,
}

/// The destination could not be chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DestinationError {
    /// Several candidate destinations and no rule picks one.
    Ambiguous(Vec<String>),
    /// The goal only mentions paths as source material; nothing is named to write.
    NoDestination(Vec<String>),
    /// The goal asks for an operation this parser does not support.
    Unsupported(String),
}

impl std::fmt::Display for DestinationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDestination(c) => write!(
                f,
                "no destination named: the goal only mentions {} as material to read; name the file to write",
                c.join(", ")
            ),
            Self::Unsupported(w) => write!(f, "{w} is not supported: name the file to create or edit"),
            Self::Ambiguous(c) => write!(
                f,
                "ambiguous destination: the goal names several files to write ({}); name one",
                c.join(", ")
            ),
        }
    }
}

fn clean(w: &str) -> &str {
    const Q: fn(char) -> bool = |c| matches!(c, '"' | '\'' | '`' | '(' | ')' | '[' | ']');
    w.trim_matches(Q)
        .trim_end_matches(['.', ',', ';', ':', '!', '?'])
        .trim_matches(Q)
}

fn has_extension(w: &str) -> bool {
    let last = w.rsplit('/').next().unwrap_or(w);
    match last.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && !ext.is_empty()
                && ext.len() <= 10
                && ext.starts_with(|c: char| c.is_ascii_alphabetic())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && !matches!(w, "e.g" | "i.e" | "etc")
                && !(ext.len() == 1 && stem.contains('.'))
        }
        None => false,
    }
}

/// True for a word that names a path, safe or not.
fn path_like(w: &str, is_file: &dyn Fn(&str) -> bool) -> bool {
    if w.is_empty() || w.contains('@') || w.contains("://") {
        return false;
    }
    let unsafe_form =
        w.starts_with('/') || w.starts_with("~/") || w == "~" || w.split('/').any(|c| c == "..");
    if unsafe_form {
        return w.contains('/') || w.starts_with('~');
    }
    let ok_chars = w
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '/'));
    ok_chars && (w.contains('/') && has_extension(w) || has_extension(w) || is_file(w))
}

/// The destination the goal names. `is_file(w)` says whether the plain
/// relative word `w` is an existing file (for extension-less names).
pub fn named_destination(
    goal: &str,
    is_file: &dyn Fn(&str) -> bool,
) -> Result<Option<String>, DestinationError> {
    let raw: Vec<&str> = goal.split_whitespace().collect();
    let words: Vec<String> = raw.iter().map(|w| clean(w).to_ascii_lowercase()).collect();
    if let Some(v) = words
        .iter()
        .find(|w| matches!(w.as_str(), "rename" | "move" | "delete" | "remove"))
    {
        return Err(DestinationError::Unsupported(v.clone()));
    }
    let mut cands: Vec<Cand> = Vec::new();
    let mut verb_since_last = false;
    let mut first_verb: Option<usize> = None;
    for (i, w) in raw.iter().enumerate() {
        let c = clean(w);
        let lw = &words[i];
        if DESTINATION_VERBS.contains(&lw.as_str()) {
            verb_since_last = true;
            first_verb.get_or_insert(i);
        }
        if !path_like(c, is_file) {
            continue;
        }
        let governed = {
            let mut j = i;
            while j > 0 && FILLERS.contains(&words[j - 1].as_str()) {
                j -= 1;
            }
            j > 0 && DESTINATION_VERBS.contains(&words[j - 1].as_str())
        };
        let host_like = !c.contains('/')
            && !is_file(c)
            && (c
                .rsplit('.')
                .next()
                .is_some_and(|t| HOST_TLDS.contains(&t.to_ascii_lowercase().as_str()))
                || (i > 0 && HOST_LEAD.contains(&words[i - 1].as_str())));
        if host_like && !governed {
            continue;
        }
        let prev = (i > 0).then(|| words[i - 1].as_str());
        let prev2 = (i > 1).then(|| words[i - 2].as_str());
        let location_of = prev == Some("of") && prev2.is_some_and(|p| LOCATIONAL.contains(&p));
        let marked = !governed
            && (prev.is_some_and(|p| SOURCE_MARKERS.contains(&p) && !location_of)
                || (prev == Some("on") && prev2 == Some("based"))
                || (prev == Some("on") && prev2 == Some("relying")));
        // Joined to the previous path-like word: "A and B", "A, B", "A or B", "A B".
        let joined = cands.last().is_some_and(|l| {
            (prev == Some("and") || prev == Some("or") || prev == Some("&")) && l.idx + 2 == i
                || l.idx + 1 == i
        });
        let source = marked || (joined && cands.last().is_some_and(|l| l.source));
        cands.push(Cand {
            path: c.to_string(),
            idx: i,
            source,
            conj: joined,
            verb_before: verb_since_last && !cands.is_empty(),
        });
        verb_since_last = false;
    }
    let dests: Vec<&Cand> = cands.iter().filter(|c| !c.source).collect();
    let Some(first) = dests.first() else {
        if cands.is_empty() {
            return Ok(None);
        }
        let mut names: Vec<String> = Vec::new();
        for c in &cands {
            if !names.contains(&c.path) {
                names.push(c.path.clone());
            }
        }
        return Err(DestinationError::NoDestination(names));
    };
    if dests.len() > 1 {
        let verb_leads = first_verb.is_some_and(|v| v < first.idx);
        let clash = dests[1..].iter().any(|d| d.conj || d.verb_before);
        if !verb_leads || clash {
            let mut names: Vec<String> = Vec::new();
            for d in &dests {
                if !names.contains(&d.path) {
                    names.push(d.path.clone());
                }
            }
            if names.len() > 1 {
                return Err(DestinationError::Ambiguous(names));
            }
        }
    }
    Ok(Some(first.path.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(g: &str) -> Result<Option<String>, DestinationError> {
        named_destination(g, &|w| w == "Makefile")
    }

    #[test]
    fn source_is_not_the_destination() {
        assert_eq!(
            d("Create docs/SUMMARY.md about README.md"),
            Ok(Some("docs/SUMMARY.md".into()))
        );
        assert_eq!(
            d("Update README.md using docs/NOTES.md"),
            Ok(Some("README.md".into()))
        );
        assert_eq!(
            d("Add a summary of README.md and LICENSE to docs/S.md"),
            Ok(Some("docs/S.md".into()))
        );
    }

    #[test]
    fn single_and_extensionless() {
        assert_eq!(d("write faq.md"), Ok(Some("faq.md".into())));
        assert_eq!(d("Edit Makefile please"), Ok(Some("Makefile".into())));
        assert_eq!(d("Edit docs now"), Ok(None));
        assert_eq!(d("say hi, e.g. twice"), Ok(None));
        assert_eq!(d("under the ## 0.1.0 heading"), Ok(None));
    }

    #[test]
    fn several_destinations_are_ambiguous() {
        assert!(matches!(
            d("Create a.md and b.md"),
            Err(DestinationError::Ambiguous(c)) if c == ["a.md", "b.md"]
        ));
        assert!(d("Create a.md. Then update b.md").is_err());
        assert!(d("put a.md b.md together").is_err());
        assert_eq!(
            d("Edit `docs/plan.txt`, then README.md"),
            Ok(Some("docs/plan.txt".into()))
        );
    }

    #[test]
    fn source_only_goals_name_no_destination() {
        for g in [
            "Summarise README.md",
            "Summarise README.md and docs/A.md",
            "Create a note summarising README.md",
        ] {
            assert!(
                matches!(d(g), Err(DestinationError::NoDestination(_))),
                "{g}"
            );
        }
        let Err(e) = d("Summarise README.md") else {
            panic!()
        };
        assert!(e.to_string().contains("README.md") && e.to_string().contains("no destination"));
    }

    #[test]
    fn verb_governing_the_path_beats_a_marker_adjective() {
        assert_eq!(d("Edit the given README.md"), Ok(Some("README.md".into())));
        assert_eq!(
            d("Update the reference README.md"),
            Ok(Some("README.md".into()))
        );
        assert_eq!(
            d("Create docs/S.md from the given README.md"),
            Ok(Some("docs/S.md".into()))
        );
    }

    #[test]
    fn host_names_are_not_files_unless_governed() {
        assert_eq!(
            d("Visit example.org and write out.md"),
            Ok(Some("out.md".into()))
        );
        assert_eq!(
            d("See docs.example.com then create out.md"),
            Ok(Some("out.md".into()))
        );
        assert_eq!(d("Create example.org"), Ok(Some("example.org".into())));
    }

    #[test]
    fn of_phrases_resolve_by_what_they_point_at() {
        let one = |g: &str| d(g).unwrap().unwrap();
        // A position inside the file being changed: the file is the destination.
        for (g, want) in [
            ("Add a line to the Intro section of README.md", "README.md"),
            ("Append a note to the end of README.md", "README.md"),
            ("Insert a badge at the top of README.md", "README.md"),
            (
                "Add \"x\" to the \"## Before a trip\" section of notes/GARAGE.md.",
                "notes/GARAGE.md",
            ),
        ] {
            assert_eq!(one(g), want, "{g}");
        }
        // Material to read: the file is a source, the destination is named elsewhere.
        for (g, want) in [
            ("Write a list of README.md into docs/L.md", "docs/L.md"),
            ("Create docs/C.md as a copy of README.md", "docs/C.md"),
            (
                "Write a summary of README.md into docs/SUMMARY.md",
                "docs/SUMMARY.md",
            ),
            // Location word in the source role: the later file is not demoted,
            // the first file after the verb still wins (documented limit).
            ("Create docs/S.md from the top of README.md", "docs/S.md"),
            (
                "Create docs/S.md summarising the section of README.md",
                "docs/S.md",
            ),
            ("Create docs/S.md about the end of README.md", "docs/S.md"),
        ] {
            assert_eq!(one(g), want, "{g}");
        }
        for g in [
            "Make a list of README.md",
            "Summarise README.md",
            "Make a copy of README.md",
        ] {
            assert!(
                matches!(d(g), Err(DestinationError::NoDestination(_))),
                "{g}"
            );
        }
    }

    #[test]
    fn rename_is_unsupported() {
        assert!(
            matches!(d("Rename a.md to b.md"), Err(DestinationError::Unsupported(w)) if w == "rename")
        );
    }

    #[test]
    fn unsafe_paths_are_named_not_skipped() {
        assert_eq!(
            d("Create the file ../outside.txt"),
            Ok(Some("../outside.txt".into()))
        );
        assert_eq!(d("Edit /etc/passwd"), Ok(Some("/etc/passwd".into())));
        assert_eq!(d("Edit ~/secret.txt"), Ok(Some("~/secret.txt".into())));
    }
}
