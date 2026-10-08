//! Requirement validation for composed documents.
//!
//! A task goal can state a measurable requirement ("in at least 20 lines",
//! "sections titled A, B and C", "covers preparing and testing"). This module
//! defines a small typed set of requirements, extracts them deterministically
//! from the goal text ([`analyze`], in `requirements_extract`), and checks the
//! COMPLETE parsed document (the content that would be saved).
//!
//! ## Two outcomes of reading a goal
//!
//! [`analyze`] returns an [`Extraction`]: the RECOGNIZED requirements, and the
//! UNCERTAIN spans. An uncertain span is goal text that looks like an explicit
//! measurable requirement (a bound word and a number next to a document
//! noun, "titled", "covers", "include the word", ...) that extraction could
//! not interpret reliably. Uncertainty is never treated as satisfied: the
//! task is refused before any model call, the report lists the spans
//! (`ComposeTaskReport::requirements_uncertain`) and nothing is approved,
//! committed or written.
//!
//! ## Supported requirements
//!
//! The exact wordings are in the table of `requirements_extract`; in short:
//! counts (`at least|at most|no fewer than|no more than|a minimum of|up to|
//! more than|fewer than|N or more|N or fewer` with digits or number words
//! one..twenty) of lines, words, list items, steps, sections/headings and
//! questions; section titles (`sections titled A, B and C`), topics
//! (`covers A, B and C`), required words and phrases, and a minimum number
//! of sentences in every section; and a markdown heading as the first line
//! (`start with a Markdown heading line that begins with "# "`).
//!
//! ## What a requirement is judged on
//!
//! The COMPLETE document that would be saved (in edit mode the merged file,
//! not the model's reply). An edit only adds lines, so a prior file that
//! already exceeds a `MaxLines` is refused before any attempt.
//!
//! `AddedLines` (a request such as "add 2 lines to README.md") is the one requirement
//! measured against the PRIOR file: the number of non-empty lines of the complete
//! proposed bytes that are not in the longest common subsequence with the
//! prior file's non-empty lines (trailing whitespace ignored). A new file is
//! compared with an empty prior. Callers bind the prior with
//! `Extraction::resolved`; an unresolved `AddedLines` is unmet (fails closed).
//! An edit may only ADD: every non-empty line of the prior file must survive in
//! order (trailing whitespace may differ), otherwise the requirement is unmet
//! whatever the number of added lines. Replacing or deleting lines is a change the
//! goal did not ask for, so "add 2 lines" can never wipe or rewrite the file.
//! The approved path binds the file found in the workspace.
//!
//! Counting: `MinLines`/`MaxLines` count every non-empty line of the file,
//! INCLUDING code lines and code-fence marker lines; `MinWords`/`MaxWords`
//! count every whitespace-separated word of the file. Sections, questions,
//! items, steps, headings, topics and sentences skip fenced code blocks
//! (``` or ~~~, CommonMark close rule: same marker, at least as long, no
//! info string; an unclosed fence runs to the end) and the fence lines
//! themselves, so a `# comment` or a `- x` inside a code block is not a
//! heading or an item.
//!
//! Definitions used by the checks:
//! - heading text: the heading line without its `#` run, leading numbering
//!   (`1.`, `2)`), emphasis marks and trailing `:.!?#`, trimmed, whitespace
//!   collapsed, compared case-insensitively.
//! - word form (topics): lowercase, strip ONE suffix of `ment ing ion es ed
//!   s` (only when 3 or more letters remain), then one trailing `e` (when 4
//!   or more letters remain). `preparing`, `prepare` and `prepared` agree;
//!   `publishing` and `public` do not. Irregular forms (`running` / `run`)
//!   do not agree: the check errs on the side of refusing.
//! - sentence: a run of at least two words ended by `.`, `!` or `?` (not
//!   inside a number such as `3.10`); an unfinished last fragment of at
//!   least three words counts as one. List markers are ignored.
//! - section (for sentence counts): a heading and the prose directly under
//!   it up to the next heading of any level. A heading with no prose of its
//!   own whose next heading is deeper is a container, not a section.

use std::collections::HashSet;

pub use crate::requirements_extract::{analyze, extract, Extraction};

/// What a `MinItems` requirement counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Items,
    Steps,
    Sections,
    Questions,
    /// Fenced code blocks (each opening fence line counts one).
    CodeBlocks,
}

impl ItemKind {
    fn noun(self) -> &'static str {
        match self {
            ItemKind::Items => "list items",
            ItemKind::Steps => "numbered steps",
            ItemKind::Sections => "headings",
            ItemKind::Questions => "questions",
            ItemKind::CodeBlocks => "fenced code blocks",
        }
    }
}

/// One measurable requirement on a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    MinLines(usize),
    MaxLines(usize),
    MinWords(usize),
    MaxWords(usize),
    MinItems(ItemKind, usize),
    /// At least `n` lines (outside fenced code, leading whitespace ignored) that
    /// start with exactly `prefix`, e.g. the `- [ ]` of a checklist.
    MinPrefixedLines {
        prefix: String,
        n: usize,
    },
    /// Case-insensitive substrings of the document.
    RequiredPhrases(Vec<String>),
    /// Whole words, case-insensitive.
    RequiredWords(Vec<String>),
    /// Section titles that must each appear as a heading.
    RequiredHeadings(Vec<String>),
    /// Topics (each a list of content words) that must each be covered: every
    /// content word must appear in the prose as a word form.
    RequiredTopics(Vec<String>),
    /// Every section holds at least this many sentences.
    MinSentencesPerSection(usize),
    /// Lines ADDED to the file, measured against the prior file (see the module
    /// docs). `exact`: exactly `n`, else at least `n`. `prior` is None until
    /// the caller resolves it ([`Extraction::resolved`]); an unresolved one is unmet.
    AddedLines {
        n: usize,
        exact: bool,
        prior: Option<String>,
    },
    /// The section with this title (None: the last section of the document)
    /// holds at least `n` words of plain text (prose lines, no code, no headings).
    SectionMinWords {
        title: Option<String>,
        n: usize,
    },
    /// Each title is present as a heading of exactly this level (1 to 6).
    LevelHeadings {
        level: usize,
        titles: Vec<String>,
    },
    /// The first line of the file is a markdown ATX heading: 1 to 6 `#` at the very start of
    /// the line, then a space or tab, then title text. `level`: exactly that many `#`;
    /// None: any of 1 to 6.
    FirstLineHeading {
        level: Option<usize>,
    },
}

fn quoted(items: &[String]) -> String {
    items
        .iter()
        .map(|x| format!("\"{x}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

impl Requirement {
    /// Short stable label, as recorded in the receipt.
    pub fn label(&self) -> String {
        match self {
            Requirement::MinLines(n) => format!("at least {n} non-empty lines"),
            Requirement::MaxLines(n) => format!("at most {n} non-empty lines"),
            Requirement::MinWords(n) => format!("at least {n} words"),
            Requirement::MaxWords(n) => format!("at most {n} words"),
            Requirement::MinItems(k, n) => format!("at least {n} {}", k.noun()),
            Requirement::MinPrefixedLines { prefix, n } => {
                format!("at least {n} lines starting with \"{prefix}\"")
            }
            Requirement::RequiredPhrases(p) => format!("the phrase {}", quoted(p)),
            Requirement::RequiredWords(p) => format!("the words {}", quoted(p)),
            Requirement::RequiredHeadings(p) => format!("the headings {}", quoted(p)),
            Requirement::RequiredTopics(p) => format!("the topics {}", quoted(p)),
            Requirement::MinSentencesPerSection(n) => {
                format!("at least {n} sentences in every section")
            }
            Requirement::AddedLines { n, exact, .. } => {
                if *exact {
                    format!("exactly {n} added non-empty lines")
                } else {
                    format!("at least {n} added non-empty lines")
                }
            }
            Requirement::SectionMinWords { title, n } => match title {
                Some(t) => format!("at least {n} words of plain text in the section \"{t}\""),
                None => format!("at least {n} words of plain text in the last section"),
            },
            Requirement::LevelHeadings { level, titles } => {
                format!("the level-{level} headings {}", quoted(titles))
            }
            Requirement::FirstLineHeading { level } => match level {
                Some(l) => format!(
                    "a level-{l} markdown heading (\"{} \") as the first line",
                    "#".repeat(*l)
                ),
                None => "a markdown heading as the first line".to_string(),
            },
        }
    }

    /// None when met, else the unmet requirement with what was found.
    pub fn check(&self, content: &str) -> Option<String> {
        let label = self.label();
        match self {
            Requirement::MinLines(n) => {
                let c = non_empty_lines(content);
                (c < *n).then(|| format!("{label}, found {c}"))
            }
            Requirement::MaxLines(n) => {
                let c = non_empty_lines(content);
                (c > *n).then(|| format!("{label}, found {c}"))
            }
            Requirement::MinWords(n) => {
                let c = content.split_whitespace().count();
                (c < *n).then(|| format!("{label}, found {c}"))
            }
            Requirement::MaxWords(n) => {
                let c = content.split_whitespace().count();
                (c > *n).then(|| format!("{label}, found {c}"))
            }
            Requirement::MinItems(k, n) => {
                let c = count_items(*k, content);
                (c < *n).then(|| format!("{label}, found {c}"))
            }
            Requirement::MinPrefixedLines { prefix, n } => {
                let c = prose_lines(content)
                    .into_iter()
                    .filter(|l| l.trim_start().starts_with(prefix.as_str()))
                    .count();
                (c < *n).then(|| format!("{label}, found {c}"))
            }
            Requirement::RequiredPhrases(p) => {
                let low = content.to_lowercase();
                let missing: Vec<String> = p
                    .iter()
                    .filter(|x| !low.contains(&x.to_lowercase()))
                    .cloned()
                    .collect();
                (!missing.is_empty()).then(|| format!("{label}, missing {}", quoted(&missing)))
            }
            Requirement::RequiredWords(p) => {
                let have: HashSet<String> = words_of(content).into_iter().collect();
                let missing: Vec<String> = p
                    .iter()
                    .filter(|x| !have.contains(&x.to_lowercase()))
                    .cloned()
                    .collect();
                (!missing.is_empty()).then(|| format!("{label}, missing {}", quoted(&missing)))
            }
            Requirement::RequiredHeadings(p) => {
                let have: HashSet<String> = parse_sections(content)
                    .into_iter()
                    .map(|s| s.title)
                    .collect();
                let missing: Vec<String> = p
                    .iter()
                    .filter(|x| !have.contains(&normalize_title(x)))
                    .cloned()
                    .collect();
                (!missing.is_empty()).then(|| {
                    format!(
                        "{label}, missing {} (each must be a markdown heading line)",
                        quoted(&missing)
                    )
                })
            }
            Requirement::RequiredTopics(p) => {
                let have: HashSet<String> = words_of(&prose_lines(content).join("\n"))
                    .iter()
                    .map(|w| stem(w))
                    .collect();
                let missing: Vec<String> = p
                    .iter()
                    .filter(|t| !content_words(t).iter().all(|w| have.contains(&stem(w))))
                    .cloned()
                    .collect();
                (!missing.is_empty()).then(|| {
                    format!(
                        "{label}, not covered: {} (use those words, or their word forms, in the text)",
                        missing.join(", ")
                    )
                })
            }
            Requirement::AddedLines { n, exact, prior } => {
                let Some(prior) = prior else {
                    return Some(format!(
                        "{label} cannot be measured: the existing file was not provided"
                    ));
                };
                let (c, lost) = match added_non_empty_lines(prior, content) {
                    Ok(c) => c,
                    Err(e) => return Some(format!("{label} cannot be measured: {e}")),
                };
                if lost > 0 {
                    return Some(format!(
                        "{label}, but {lost} existing non-empty line(s) were changed or removed (every existing line must stay, in order; only trailing whitespace may differ)"
                    ));
                }
                let bad = if *exact { c != *n } else { c < *n };
                bad.then(|| format!("{label}, found {c}"))
            }
            Requirement::SectionMinWords { title, n } => {
                let secs = parse_sections(content);
                let sec = match title {
                    Some(t) => secs.iter().find(|s| s.title == normalize_title(t)),
                    None => secs.last(),
                };
                let Some(sec) = sec else {
                    return Some(format!("{label}, the section was not found"));
                };
                let c = sec
                    .body
                    .split_whitespace()
                    .filter(|w| w.chars().any(char::is_alphanumeric))
                    .count();
                (c < *n).then(|| format!("{label}, found {c}"))
            }
            Requirement::LevelHeadings { level, titles } => {
                let secs = parse_sections(content);
                let missing: Vec<String> = titles
                    .iter()
                    .filter(|t| {
                        !secs
                            .iter()
                            .any(|s| s.level == *level && s.title == normalize_title(t))
                    })
                    .cloned()
                    .collect();
                (!missing.is_empty()).then(|| {
                    format!(
                        "{label}, missing {} (each must be a level-{level} markdown heading line)",
                        quoted(&missing)
                    )
                })
            }
            Requirement::FirstLineHeading { level } => {
                let first = content.split('\n').next().unwrap_or("");
                let first = first.strip_suffix('\r').unwrap_or(first);
                let hashes = first.bytes().take_while(|&b| b == b'#').count();
                let has_title = first[hashes..]
                    .strip_prefix([' ', '\t'])
                    .is_some_and(|t| !t.trim().trim_end_matches('#').trim().is_empty());
                let ok =
                    (1..=6).contains(&hashes) && level.is_none_or(|l| l == hashes) && has_title;
                (!ok).then(|| {
                    let shown: String = first.chars().take(60).collect();
                    if shown.is_empty() {
                        format!("{label}, found an empty first line")
                    } else {
                        format!("{label}, found \"{shown}\"")
                    }
                })
            }
            Requirement::MinSentencesPerSection(n) => {
                let secs = parse_sections(content);
                if secs.is_empty() {
                    return Some(format!("{label}, found no headings"));
                }
                let short: Vec<String> = secs
                    .iter()
                    .filter(|s| !s.container)
                    .filter_map(|s| {
                        let c = count_sentences(&s.body);
                        (c < *n).then(|| format!("{} ({c})", s.raw_title))
                    })
                    .collect();
                (!short.is_empty()).then(|| {
                    let shown: Vec<String> = short.iter().take(8).cloned().collect();
                    format!("{label}, too few in: {}", shown.join(", "))
                })
            }
        }
    }
}

fn non_empty_lines(s: &str) -> usize {
    s.lines().filter(|l| !l.trim().is_empty()).count()
}

/// Lowercase alphanumeric words (apostrophes kept inside a word).
fn words_of(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(|w| w.trim_matches('\'').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

const STOPWORDS: [&str; 30] = [
    "a", "an", "the", "of", "to", "for", "and", "or", "in", "on", "with", "your", "you", "our",
    "their", "its", "it", "is", "are", "be", "as", "at", "by", "from", "into", "about", "this",
    "that", "new", "all",
];

/// The content words of a topic: its words minus the small stop list.
pub(crate) fn content_words(topic: &str) -> Vec<String> {
    words_of(topic)
        .into_iter()
        .filter(|w| !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// Conservative word form (see the module docs).
pub(crate) fn stem(w: &str) -> String {
    let mut s = w.to_lowercase();
    for suf in ["ment", "ing", "ion", "es", "ed", "s"] {
        if s.len() >= suf.len() + 3 && s.ends_with(suf) {
            s.truncate(s.len() - suf.len());
            break;
        }
    }
    if s.len() >= 4 && s.ends_with('e') {
        s.pop();
    }
    s
}

/// Heading text as compared: see the module docs.
pub(crate) fn normalize_title(t: &str) -> String {
    let t: String = t
        .chars()
        .filter(|c| !matches!(c, '*' | '_' | '`'))
        .collect();
    let t = t.trim().trim_end_matches('#').trim();
    let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
    let t = if d > 0 && matches!(t[d..].chars().next(), Some('.') | Some(')')) {
        t[d + 1..].trim_start()
    } else {
        t
    };
    let t = t.trim_end_matches([':', '.', '!', '?']);
    t.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

struct Section {
    /// Normalized title (compared) and the title as written (reported).
    title: String,
    raw_title: String,
    /// Prose directly under the heading, headings excluded.
    body: String,
    /// No prose of its own and the next heading is deeper.
    container: bool,
    /// Heading level, 1 to 6.
    level: usize,
}

fn heading_of(t: &str) -> Option<(usize, &str)> {
    let t = t.trim();
    let h = t.chars().take_while(|&c| c == '#').count();
    ((1..=6).contains(&h) && t[h..].starts_with(' ') && !t[h..].trim().is_empty())
        .then(|| (h, t[h..].trim()))
}

fn parse_sections(content: &str) -> Vec<Section> {
    let lines = prose_lines(content);
    let heads: Vec<(usize, usize, &str)> = lines
        .iter()
        .enumerate()
        .filter_map(|(i, l)| heading_of(l).map(|(lv, t)| (i, lv, t)))
        .collect();
    heads
        .iter()
        .enumerate()
        .map(|(k, &(i, lv, t))| {
            let end = heads.get(k + 1).map_or(lines.len(), |h| h.0);
            let body = lines[i + 1..end].join("\n");
            let next_deeper = heads.get(k + 1).is_some_and(|n| n.1 > heads[k].1);
            Section {
                title: normalize_title(t),
                raw_title: t.trim_end_matches('#').trim().to_string(),
                container: body.trim().is_empty() && next_deeper,
                level: lv,
                body,
            }
        })
        .collect()
}

fn count_sentences(body: &str) -> usize {
    let mut text = String::new();
    for l in body.lines() {
        let mut t = l.trim();
        if let Some(r) = ["- ", "* ", "+ "].iter().find_map(|m| t.strip_prefix(m)) {
            t = r;
        } else {
            let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
            if d > 0
                && matches!(t[d..].chars().next(), Some('.') | Some(')'))
                && t[d + 1..].starts_with(' ')
            {
                t = &t[d + 2..];
            }
        }
        text.push_str(t);
        text.push(' ');
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut count, mut cur) = (0usize, String::new());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        cur.push(c);
        if matches!(c, '.' | '!' | '?') {
            let mut j = i + 1;
            while j < chars.len() && matches!(chars[j], '.' | '!' | '?' | '"' | '\'' | ')') {
                cur.push(chars[j]);
                j += 1;
            }
            if j >= chars.len() || chars[j].is_whitespace() {
                if cur.split_whitespace().count() >= 2 {
                    count += 1;
                }
                cur.clear();
                i = j;
                continue;
            }
            i = j;
            continue;
        }
        i += 1;
    }
    if cur.split_whitespace().count() >= 3 {
        count += 1;
    }
    count
}

fn numbered(t: &str) -> bool {
    let d = t.chars().take_while(|c| c.is_ascii_digit()).count();
    d > 0 && matches!(t[d..].chars().next(), Some('.') | Some(')')) && t[d + 1..].starts_with(' ')
}

/// A CommonMark code-fence line: its marker character (backtick or tilde),
/// run length (3 or more) and whether an info string follows the run. A
/// backtick fence whose info string holds a backtick is not a fence.
fn fence_marker(l: &str) -> Option<(char, usize, bool)> {
    let t = l.trim_start();
    let c = t.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let n = t.chars().take_while(|&x| x == c).count();
    if n < 3 {
        return None;
    }
    let info = t[n..].trim();
    if c == '`' && info.contains('`') {
        return None;
    }
    Some((c, n, !info.is_empty()))
}

/// The lines of `s` that are NOT inside a fenced code block and are not a
/// fence marker line. A block opens at a fence line and closes at the first
/// later line with the same marker character, at least as long a run and no
/// info string (CommonMark); an unclosed block runs to the end.
fn prose_lines(s: &str) -> Vec<&str> {
    let mut open: Option<(char, usize)> = None;
    let mut out = Vec::new();
    for l in s.lines() {
        match (open, fence_marker(l)) {
            (None, Some((c, n, _))) => open = Some((c, n)),
            (None, None) => out.push(l),
            (Some((c, n)), Some((c2, n2, info))) if c2 == c && n2 >= n && !info => open = None,
            (Some(_), _) => {}
        }
    }
    out
}

fn count_items(k: ItemKind, s: &str) -> usize {
    if k == ItemKind::CodeBlocks {
        return count_code_blocks(s);
    }
    prose_lines(s)
        .into_iter()
        .map(str::trim)
        .filter(|t| match k {
            ItemKind::Items => {
                t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") || numbered(t)
            }
            ItemKind::Steps => numbered(t),
            ItemKind::Sections => {
                let h = t.chars().take_while(|&c| c == '#').count();
                (1..=6).contains(&h) && t[h..].starts_with(' ') && !t[h..].trim().is_empty()
            }
            ItemKind::Questions => t.ends_with('?'),
            ItemKind::CodeBlocks => false,
        })
        .count()
}

/// Every unmet requirement, each as a short sentence. Empty = all met.
pub fn unmet(reqs: &[Requirement], content: &str) -> Vec<String> {
    reqs.iter().filter_map(|r| r.check(content)).collect()
}

/// The refusal reason a retry prompt carries; None when all are met.
pub fn refusal_reason(reqs: &[Requirement], content: &str) -> Option<String> {
    let u = unmet(reqs, content);
    (!u.is_empty()).then(|| format!("unmet requirement: {}", u.join("; ")))
}

/// Fenced code blocks: every opening fence line counts one (same fence rule as
/// `prose_lines`; an unclosed block counts once).
fn count_code_blocks(s: &str) -> usize {
    let mut open: Option<(char, usize)> = None;
    let mut blocks = 0;
    for l in s.lines() {
        match (open, fence_marker(l)) {
            (None, Some((c, n, _))) => {
                open = Some((c, n));
                blocks += 1;
            }
            (Some((c, n)), Some((c2, n2, info))) if c2 == c && n2 >= n && !info => open = None,
            _ => {}
        }
    }
    blocks
}

/// Largest line-pair table the added-line diff will build.
const DIFF_MAX_CELLS: usize = 25_000_000;

/// (added, lost): `added` counts the non-empty lines of `content` outside the longest
/// common subsequence with the non-empty lines of `prior` (lines compared with
/// trailing whitespace ignored); `lost` counts the prior non-empty lines outside it,
/// i.e. changed or removed ones. For an edit, which keeps every prior line,
/// this is the number of lines the edit added; for whole replacement bytes it
/// is the diff against the file they replace.
pub(crate) fn added_non_empty_lines(prior: &str, content: &str) -> Result<(usize, usize), String> {
    let a: Vec<&str> = prior
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let b: Vec<&str> = content
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .collect();
    if a.len().saturating_mul(b.len()) > DIFF_MAX_CELLS {
        return Err(format!(
            "the diff is too large ({} prior lines x {} lines)",
            a.len(),
            b.len()
        ));
    }
    // Rolling-row longest common subsequence length.
    let mut row = vec![0usize; b.len() + 1];
    for x in &a {
        let mut diag = 0;
        for (j, y) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = if x == y { diag + 1 } else { up.max(row[j]) };
            diag = up;
        }
    }
    let common = row[b.len()];
    Ok((b.len() - common, a.len() - common))
}
