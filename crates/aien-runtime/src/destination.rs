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
}

impl std::fmt::Display for DestinationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
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
        let prev = (i > 0).then(|| words[i - 1].as_str());
        let prev2 = (i > 1).then(|| words[i - 2].as_str());
        let marked = prev.is_some_and(|p| SOURCE_MARKERS.contains(&p))
            || (prev == Some("on") && prev2 == Some("based"))
            || (prev == Some("on") && prev2 == Some("relying"));
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
        return Ok(None);
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
    fn unsafe_paths_are_named_not_skipped() {
        assert_eq!(
            d("Create the file ../outside.txt"),
            Ok(Some("../outside.txt".into()))
        );
        assert_eq!(d("Edit /etc/passwd"), Ok(Some("/etc/passwd".into())));
        assert_eq!(d("Edit ~/secret.txt"), Ok(Some("~/secret.txt".into())));
    }
}
