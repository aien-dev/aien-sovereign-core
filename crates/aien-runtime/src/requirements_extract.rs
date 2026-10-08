//! Reading the requirements out of a goal (see `requirements` for the model).
//!
//! `analyze` is rule based and deterministic. It returns the requirements it
//! recognized AND the spans of goal text that look like explicit measurable
//! requirements but could not be interpreted reliably (`Extraction::uncertain`).
//! A caller must refuse a task with uncertain spans: uncertainty is never
//! treated as satisfied.
//!
//! ## Recognized wordings (case-insensitive; trailing punctuation on a word is ignored)
//!
//! | goal wording | requirement |
//! |---|---|
//! | `at least / no fewer than / no less than / not fewer than / a minimum of / minimum of / min N <noun>`, `N or more <noun>`, `N+ <noun>` | `MinX(N)` |
//! | `at most / no more than / not more than / a maximum of / maximum of / max / up to N <noun>`, `N or fewer|less <noun>` | `MaxX(N)` |
//! | `more than N <noun>`, `fewer|less than N <noun>` | `MinX(N+1)`, `MaxX(N-1)` |
//! | `N <noun> or more|fewer|less`, `N <noun> minimum|maximum`, `no longer|shorter than N <noun>` | `MinX(N)` or `MaxX(N)` |
//! | `<noun>` = `lines`, `words`; `items`, `steps`, `sections`, `headings`, `questions` (Min only; singular too) | lines, words, list items, numbered steps, headings, questions |
//! | `N sentences in|per|for|within every|each section` (or `in every section, ... N sentences`) | `MinSentencesPerSection(N)` |
//! | `sections|headings titled|named|called A, B and C`, `sections: A, B and C` | `RequiredHeadings` |
//! | `covers|covering A, B and C` (and `to|should|must|will|can cover ...`) | `RequiredTopics` |
//! | `[must|...] cover|covers|covering N topics: a, b and c` (N equals the list, which ends with the item after its `and`/`or`); a later `... form ... for example X or Y for the first|second|...|last [one]` adds the named forms the word-form rule does not already accept, as `topic (or X)` | `RequiredTopics` |
//! | `include|contain|use|mention the word(s)|term(s) X, Y` or `"X", "Y"` | `RequiredWords` |
//! | `include|contain|use|mention the phrase "X"` / `the phrases "X", "Y"` | `RequiredPhrases` |
//! | `add|append|insert|with N line(s)`, `exactly N lines` after such a verb, `a N-line <thing>` after a verb | `AddedLines` exactly N |
//! | `write|put|create|draft|compose|produce|generate N line(s)` | `AddedLines` at least N |
//! | `<bound> N lines is|are needed|required|necessary` (a trailing status word) | the same as the bound without it |
//! | `... section titled "T" that holds at least N words of plain text`, `the last section ... N words of plain text` | `SectionMinWords` |
//! | `N level-two|level-2|second-level|h2 sections titled A, B and C` | `LevelHeadings` (those headings at that level; extra headings of the level are allowed) |
//! | `start|begin|open with a [markdown] [level-N] heading [line]`, then the end of the clause or `that begins|starts with "# "` (quoted, 1 to 6 `#` and one space) | `FirstLineHeading` (the first line of the file is that heading; a marker fixes the level). Negated, emphasised, qualified or unquoted forms, and any other heading wording after `start with a`, are UNCERTAIN |
//! | `include|add|put|provide N [separate] fenced code examples|blocks`, or a bound cue before N | `MinItems(CodeBlocks)` |
//!
//! Bare line counts. The VERB decides the reading, no task wording is built in. An
//! edit (the goal names an existing file) is judged on the diff between that file
//! and the exact proposed bytes (non-empty lines of the proposal that are not in
//! the longest common subsequence with the file's non-empty lines); a new file
//! counts as an edit of the empty file. `add 2 lines` therefore means exactly two
//! non-empty lines were added; `write 20 lines` at least twenty. The count must end
//! its clause or be followed by `to in into at for about on saying describing giving
//! listing ...`; `3 lines of context`, `10 lines per section` stay UNCERTAIN.
//! "N-sentence" and "one-sentence" are NOT read: a sentence count over a whole document
//! cannot be verified reliably (headings, lists, abbreviations), so such a goal
//! is UNCERTAIN and refused. `N words of plain text` without a named section
//! (`section titled "T"`, `the last section`) is a count over an unknown scope and
//! is UNCERTAIN. `cover three topics: a, b and c` is read only when the count equals
//! the list (sc#332); otherwise it is UNCERTAIN.
//! "N lines at least" is a lower bound; "a single line" is exactly one. Left UNCERTAIN on
//! purpose: ranges ("2-3 lines"), "a dozen lines", "three-plus sections", "line count at
//! least 40", counts of shell commands or snippets, level-two headings given as a bare
//! list, and a section title that itself holds a count. "a couple of", "a few",
//! "several" and "add a line" (no number) state no number and stay silent.
//! The machine goal of an approved run ("apply approved proposal for <path> ...")
//! is never analysed; the goal bound into the approval is.
//!
//! `N` is digits or a number word (`one` to `twenty`, tens up to `hundred`,
//! `twenty-five`), at least 1.
//!
//! Safety net: after the recognizers, any number (digits or one..hundred) within three
//! words of a countable document noun (line, word, sentence, paragraph, section,
//! heading, bullet, point, item, step, block, example, question, topic, character, page,
//! table, row, column), a quantity cue (`twice minimum maximum limit or more/fewer no
//! longer than`) near such a noun, and heading or word list cues without a number
//! (`headings for`, `a Summary heading`, `these words:`) that no recognizer consumed is
//! UNCERTAIN. A bare count ("Keep it to 10 lines", "three sections") is therefore
//! uncertain, never silent. Numbers without such a noun ("Fix the 2 typos") are ignored.
//!
//! Rules that keep it conservative:
//! - the counted noun must END its clause: closing punctuation, end of goal, a
//!   joiner (`and or but so then because that which covering`) or one of
//!   `long with total overall titled named called about on including
//!   containing`, or `in total`. "3 lines of context", "5 items per category",
//!   "at least 4 questions in the survey" qualify the count and are UNCERTAIN.
//! - a negator (`not no never without avoid` and contractions) up to 4 words
//!   before, in the same clause, makes the span UNCERTAIN (never inverted).
//! - emphasis or quotes around the wording (`**at least 20 lines**`) are not
//!   recognized and the span is UNCERTAIN.
//! - a bound word with a number and a noun that is not supported (`at most 5
//!   items`, `at least 3 paragraphs`, `exactly 20 lines`, `about 30 lines`,
//!   `between 10 and 20 lines`, `35-line` with no verb before it) is UNCERTAIN.
//! - a title list whose parsed length differs from a declared count ("6
//!   sections titled A, B, C") is UNCERTAIN; so is a list item that is not a
//!   short noun phrase.
//! - the title or topic list is the text after the key word up to the end of
//!   the sentence (or `, each|with|which|that|...`, or a count bound); a
//!   trailing `and` splits the LAST comma item (`Who to ask and Glossary`),
//!   so a title that itself contains `and` is only safe inside the list.

use crate::requirements::{content_words, topic_forms, word_keys, ItemKind, Requirement};

/// What a goal says: recognized requirements and the spans it could not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extraction {
    pub requirements: Vec<Requirement>,
    /// Goal text that looks like an explicit measurable requirement but could
    /// not be interpreted reliably. Non-empty = the task must be refused.
    pub uncertain: Vec<String>,
}

impl Extraction {
    /// True when a requirement is measured against the prior file
    /// ([`Requirement::AddedLines`]) and so needs [`Extraction::resolved`].
    pub fn needs_prior(&self) -> bool {
        self.requirements
            .iter()
            .any(|r| matches!(r, Requirement::AddedLines { .. }))
    }

    /// The same extraction with every added-line requirement bound to the
    /// content of the file the proposal replaces (`""` for a new file).
    pub fn resolved(mut self, prior: &str) -> Self {
        for r in &mut self.requirements {
            if let Requirement::AddedLines { prior: p, .. } = r {
                *p = Some(prior.to_string());
            }
        }
        self
    }

    /// The refusal reason when any span is uncertain.
    pub fn refusal(&self) -> Option<String> {
        (!self.uncertain.is_empty()).then(|| {
            let spans: Vec<String> = self.uncertain.iter().map(|s| format!("\"{s}\"")).collect();
            format!(
                "uncertain requirement: the goal states a measurable requirement this runtime could not interpret reliably, so nothing was proposed: {}",
                spans.join("; ")
            )
        })
    }
}

/// Only the recognized requirements (see [`analyze`] for the uncertain spans).
pub fn extract(goal: &str) -> Vec<Requirement> {
    analyze(goal).requirements
}

struct Tok {
    /// Lowercase with its punctuation.
    raw: String,
    /// `raw` without surrounding clause punctuation.
    word: String,
    /// `word` without emphasis and quote marks too.
    norm: String,
    start: usize,
    end: usize,
}

fn clean(w: &str) -> &str {
    w.trim_matches(|c: char| matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '(' | ')'))
}

fn plain(w: &str) -> &str {
    w.trim_matches(|c: char| {
        matches!(
            c,
            ',' | '.' | ';' | ':' | '!' | '?' | '(' | ')' | '*' | '_' | '"' | '\'' | '`'
        )
    })
}

fn tokenize(goal: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    let push = |out: &mut Vec<Tok>, s: usize, e: usize| {
        let raw = goal[s..e].to_ascii_lowercase();
        out.push(Tok {
            word: clean(&raw).to_string(),
            norm: plain(&raw).to_string(),
            raw,
            start: s,
            end: e,
        });
    };
    for (i, c) in goal.char_indices() {
        if c.is_whitespace() {
            if let Some(s) = start.take() {
                push(&mut out, s, i);
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(s) = start {
        push(&mut out, s, goal.len());
    }
    out
}

/// A word that ends its clause: it carries closing punctuation.
fn ends_clause(raw: &str) -> bool {
    raw.ends_with([',', '.', ';', ':', '!', '?', ')'])
}

/// Words allowed right after the counted noun or the quoted phrase: they
/// join a new clause instead of qualifying the count.
const CLAUSE_JOINERS: [&str; 9] = [
    "and", "or", "but", "so", "then", "because", "that", "which", "covering",
];

/// Other words that may follow the counted noun without changing its meaning.
const NOUN_FOLLOWERS: [&str; 11] = [
    "please", "thanks", "long", "with", "total", "overall", "titled", "named", "called", "about",
    "on",
];
const NOUN_FOLLOWERS_2: [&str; 2] = ["including", "containing"];

/// Words that turn a requirement into its opposite or into something else.
const NEGATORS: [&str; 20] = [
    "not",
    "no",
    "never",
    "without",
    "avoid",
    "dont",
    "don't",
    "doesnt",
    "doesn't",
    "cant",
    "can't",
    "cannot",
    "wont",
    "won't",
    "shouldnt",
    "shouldn't",
    "mustnt",
    "mustn't",
    "isnt",
    "isn't",
];

/// True when one of the (at most 4) words before position `at` in the same
/// clause is a negator. `raw` are the whitespace-split lowercase words.
fn negated_before(toks: &[Tok], at: usize) -> bool {
    for i in (at.saturating_sub(4)..at).rev() {
        if ends_clause(&toks[i].raw) {
            return false;
        }
        if NEGATORS.contains(&toks[i].word.as_str()) {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dir {
    Min,
    Max,
}

/// (cue words, direction, adjustment: +1 strict min, -1 strict max).
const CUES: [(&[&str], Dir, i64); 22] = [
    (&["no", "longer", "than"], Dir::Max, 0),
    (&["no", "shorter", "than"], Dir::Min, 0),
    (&["no", "fewer", "than"], Dir::Min, 0),
    (&["no", "less", "than"], Dir::Min, 0),
    (&["not", "fewer", "than"], Dir::Min, 0),
    (&["not", "less", "than"], Dir::Min, 0),
    (&["no", "more", "than"], Dir::Max, 0),
    (&["not", "more", "than"], Dir::Max, 0),
    (&["a", "minimum", "of"], Dir::Min, 0),
    (&["a", "maximum", "of"], Dir::Max, 0),
    (&["minimum", "of"], Dir::Min, 0),
    (&["maximum", "of"], Dir::Max, 0),
    (&["at", "least"], Dir::Min, 0),
    (&["at", "most"], Dir::Max, 0),
    (&["more", "than"], Dir::Min, 1),
    (&["fewer", "than"], Dir::Max, -1),
    (&["less", "than"], Dir::Max, -1),
    (&["up", "to"], Dir::Max, 0),
    (&["minimum"], Dir::Min, 0),
    (&["maximum"], Dir::Max, 0),
    (&["min"], Dir::Min, 0),
    (&["max"], Dir::Max, 0),
];

/// Cue words that state a bound or a size this module does not interpret.
const VAGUE: [&str; 11] = [
    "exactly",
    "exact",
    "about",
    "around",
    "approximately",
    "roughly",
    "nearly",
    "between",
    "over",
    "under",
    "below",
];

const NOUNS: [&str; 46] = [
    "topic",
    "topics",
    "block",
    "blocks",
    "table",
    "tables",
    "column",
    "columns",
    "line",
    "lines",
    "word",
    "words",
    "item",
    "items",
    "step",
    "steps",
    "section",
    "sections",
    "heading",
    "headings",
    "question",
    "questions",
    "sentence",
    "sentences",
    "paragraph",
    "paragraphs",
    "bullet",
    "bullets",
    "point",
    "points",
    "entry",
    "entries",
    "row",
    "rows",
    "example",
    "examples",
    "page",
    "pages",
    "chapter",
    "chapters",
    "character",
    "characters",
    "command",
    "commands",
    "snippet",
    "snippets",
];

/// Verbs that introduce a bare line count (`true` = exactly N added lines).
fn line_verb(w: &str) -> Option<bool> {
    match w {
        "add" | "append" | "insert" | "with" => Some(true),
        "write" | "put" | "create" | "draft" | "compose" | "produce" | "generate" => Some(false),
        _ => None,
    }
}

/// Words that may follow "N lines" without qualifying the count.
const LINE_TAILS: [&str; 22] = [
    "to",
    "in",
    "into",
    "at",
    "for",
    "saying",
    "describing",
    "giving",
    "listing",
    "explaining",
    "stating",
    "that",
    "which",
    "under",
    "after",
    "before",
    "below",
    "above",
    "telling",
    "naming",
    "mentioning",
    "summarizing",
];

const CODE_NOUNS: [&str; 6] = [
    "example", "examples", "block", "blocks", "snippet", "snippets",
];
const CODE_ADJ: [&str; 13] = [
    "separate",
    "fenced",
    "code",
    "shell",
    "bash",
    "distinct",
    "different",
    "small",
    "short",
    "runnable",
    "sample",
    "markdown",
    "command-line",
];
const CODE_VERBS: [&str; 12] = [
    "include", "includes", "add", "put", "provide", "give", "show", "use", "with", "contain",
    "contains", "have",
];

const SECTION_NOUNS: [&str; 8] = [
    "sections", "section", "headings", "heading", "chapters", "chapter", "parts", "part",
];

#[derive(Debug, PartialEq, Eq)]
enum Num {
    Val(usize),
    /// Looks like a number (starts with a digit) but is zero or unparseable.
    Bad,
    Not,
}

fn small(w: &str) -> Option<usize> {
    const ONES: [&str; 20] = [
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
    ];
    const TENS: [(&str, usize); 8] = [
        ("thirty", 30),
        ("forty", 40),
        ("fifty", 50),
        ("sixty", 60),
        ("seventy", 70),
        ("eighty", 80),
        ("ninety", 90),
        ("hundred", 100),
    ];
    if let Some(i) = ONES.iter().position(|x| *x == w) {
        return Some(i + 1);
    }
    TENS.iter().find(|(x, _)| *x == w).map(|(_, v)| *v)
}

fn parse_num(w: &str) -> Num {
    if w.is_empty() {
        return Num::Not;
    }
    if w.chars().all(|c| c.is_ascii_digit()) {
        return match w.parse::<usize>() {
            Ok(n) if n > 0 => Num::Val(n),
            _ => Num::Bad,
        };
    }
    if let Some(n) = small(w) {
        return Num::Val(n);
    }
    if let Some((t, o)) = w.split_once('-') {
        if let (Some(t), Some(o)) = (small(t), small(o)) {
            if t >= 20 && t % 10 == 0 && t < 100 && o < 10 {
                return Num::Val(t + o);
            }
        }
    }
    if w.starts_with(|c: char| c.is_ascii_digit())
        || w.strip_prefix('-')
            .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
    {
        return Num::Bad;
    }
    Num::Not
}

/// Match a cue at `s`; (length in tokens, direction, adjustment).
fn match_cue(toks: &[Tok], s: usize, norm: bool) -> Option<(usize, Dir, i64)> {
    let w = |i: usize| -> Option<&str> {
        toks.get(i).map(|t| {
            if norm {
                t.norm.as_str()
            } else {
                t.word.as_str()
            }
        })
    };
    CUES.iter().find_map(|(words, dir, adj)| {
        words
            .iter()
            .enumerate()
            .all(|(k, x)| w(s + k) == Some(*x))
            .then_some((words.len(), *dir, *adj))
    })
}

fn qualifier_ok(toks: &[Tok], j: usize) -> bool {
    if ends_clause(&toks[j].raw) {
        return true;
    }
    match toks.get(j + 1) {
        None => true,
        Some(n) => {
            let w = n.word.as_str();
            let status = |s: &str| {
                matches!(
                    s,
                    "needed" | "required" | "necessary" | "expected" | "enough" | "sufficient"
                )
            };
            status(w)
                || (matches!(w, "is" | "are")
                    && toks.get(j + 2).is_some_and(|t| status(t.word.as_str())))
                || CLAUSE_JOINERS.contains(&w)
                || NOUN_FOLLOWERS.contains(&w)
                || NOUN_FOLLOWERS_2.contains(&w)
                || (w == "in" && toks.get(j + 2).is_some_and(|t| clean(&t.raw) == "total"))
        }
    }
}

/// Mark the tokens that start inside the byte range as consumed.
fn mark_bytes(handled: &mut std::collections::BTreeSet<usize>, toks: &[Tok], lo: usize, hi: usize) {
    for (i, t) in toks.iter().enumerate() {
        if t.start >= lo && t.start < hi {
            handled.insert(i);
        }
    }
}

/// True when the text holds a number next to a countable noun ("5 steps").
fn has_count_and_noun(t: &str) -> bool {
    let w: Vec<String> = t
        .split_whitespace()
        .map(|x| plain(&x.to_ascii_lowercase()).to_string())
        .collect();
    (0..w.len()).any(|i| {
        matches!(parse_num(&w[i]), Num::Val(_))
            && (i + 1..=i + 2).any(|j| w.get(j).is_some_and(|x| NOUNS.contains(&x.as_str())))
    })
}

/// Text of the goal from token `from` to the end of its clause (at most 10 tokens).
fn snippet(goal: &str, toks: &[Tok], from: usize, commas: bool) -> String {
    let ends = |raw: &str| {
        if commas {
            ends_clause(raw)
        } else {
            raw.ends_with(['.', '!', '?', ';'])
        }
    };
    let mut to = from;
    while to + 1 < toks.len() && to < from + 9 && !ends(&toks[to].raw) {
        to += 1;
    }
    goal[toks[from].start..toks[to].end]
        .trim_end_matches([',', '.', ';', ':', '!', '?'])
        .to_string()
}

/// Sentence-counting words after the noun: `in|per|for|within [every|each|a|the|any] section(s)`.
fn per_section_after(toks: &[Tok], j: usize) -> Option<usize> {
    let w = |i: usize| toks.get(i).map(|t| t.word.as_str());
    if !matches!(w(j + 1), Some("in" | "per" | "for" | "within" | "under")) {
        return None;
    }
    let mut k = j + 2;
    if matches!(w(k), Some("every" | "each" | "a" | "the" | "any")) {
        k += 1;
    }
    matches!(w(k), Some("section" | "sections" | "heading" | "headings")).then_some(k)
}

/// "In every section, write ... N sentences": every|each section earlier in the sentence.
fn every_section_before(toks: &[Tok], s: usize) -> bool {
    for m in (s.saturating_sub(10)..s).rev() {
        if m >= 1
            && matches!(toks[m - 1].word.as_str(), "every" | "each")
            && matches!(toks[m].word.as_str(), "section" | "heading")
        {
            return true;
        }
        if ends_clause(&toks[m].raw)
            && m + 1 != s
            && !matches!(toks[m].word.as_str(), "section" | "heading")
        {
            return false;
        }
    }
    false
}

/// The text after a key word up to the end of its clause.
fn list_region(goal: &str, from: usize) -> &str {
    let rest = &goal[from..];
    let b = rest.as_bytes();
    let mut end = rest.len();
    for (i, c) in rest.char_indices() {
        if matches!(c, '.' | '!' | '?' | ';' | '\n')
            && (i + 1 >= b.len() || (b[i + 1] as char).is_whitespace())
        {
            end = i;
            break;
        }
    }
    let mut region = &rest[..end];
    let low = region.to_ascii_lowercase();
    let mut cut = region.len();
    for m in [
        ", each ",
        ", with ",
        ", which ",
        ", that ",
        ", where ",
        ", but ",
        ", so ",
        ", because ",
        ", then ",
        ", and each ",
        ", and write ",
        ", and make ",
        " at least ",
        " at most ",
        " no more than ",
        " no fewer than ",
        " a minimum of ",
        " up to ",
    ] {
        if let Some(i) = low.find(m) {
            cut = cut.min(i);
        }
    }
    region = &region[..cut];
    // A bound often follows as "... in|of|with|and at least N": drop the dangling joiner.
    let t = region.trim_end();
    for tail in [" in", " of", " with", " and", " or", " within"] {
        if t.to_ascii_lowercase().ends_with(tail) {
            return &t[..t.len() - tail.len()];
        }
    }
    t
}

fn quoted_items(region: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut open: Option<usize> = None;
    for (i, c) in region.char_indices() {
        match (c, open) {
            ('"' | '\u{201C}', None) => open = Some(i + c.len_utf8()),
            ('"' | '\u{201D}', Some(s)) => {
                let t = region[s..i].trim();
                if !t.is_empty() {
                    out.push(t.to_string());
                }
                open = None;
            }
            _ => {}
        }
    }
    out
}

/// "of plain text", "of prose", "of body text" after a word count: the index of
/// the last token of that phrase.
fn plain_text_after(toks: &[Tok], j: usize) -> Option<usize> {
    let w = |i: usize| toks.get(i).map(|t| t.word.as_str());
    if w(j + 1) != Some("of") {
        return None;
    }
    let mut k = j + 2;
    if matches!(w(k), Some("plain" | "body" | "running" | "normal")) {
        k += 1;
    }
    matches!(w(k), Some("text" | "prose" | "writing" | "plain-text")).then_some(k)
}

/// The section a sentence names before token `s`: `Some(None)` for "the last
/// (or final) section", `Some(Some(title))` for `section titled "Title"` (a
/// quoted title, or up to four plain words), None when the sentence names no
/// section.
fn section_scope_before(goal: &str, toks: &[Tok], s: usize) -> Option<Option<String>> {
    let mut m = s;
    while m > 0 {
        m -= 1;
        if m + 1 < s && toks[m].raw.ends_with(['.', '!', '?']) {
            return None;
        }
        let w = toks[m].word.as_str();
        if matches!(w, "last" | "final")
            && toks
                .get(m + 1)
                .is_some_and(|t| matches!(t.word.as_str(), "section" | "heading"))
        {
            return Some(None);
        }
        if matches!(w, "titled" | "named" | "called")
            && m >= 1
            && matches!(toks[m - 1].word.as_str(), "section" | "heading")
        {
            let rest = goal[toks[m].end..].trim_start();
            if rest.starts_with(['"', '\u{201C}']) {
                let (q, _) = leading_quoted_items(rest);
                return q.first().map(|t| Some(t.clone()));
            }
            let title: Vec<&str> = toks[m + 1..s]
                .iter()
                .take_while(|t| {
                    !matches!(
                        t.word.as_str(),
                        "that"
                            | "which"
                            | "with"
                            | "who"
                            | "where"
                            | "holds"
                            | "has"
                            | "and"
                            | "contains"
                            | "having"
                    ) && !t.raw.ends_with([',', '.', ';', ':'])
                })
                .map(|t| t.raw.as_str())
                .collect();
            return (!title.is_empty() && title.len() <= 4).then(|| Some(title.join(" ")));
        }
    }
    None
}

/// The heading level an adjective names: `level-two`, `level-2`, `second-level`, `h2`.
fn level_adj(w: &str) -> Option<usize> {
    let lv = |v: usize| (1..=6).contains(&v).then_some(v);
    if let Some(r) = w.strip_prefix("level-") {
        return match parse_num(r) {
            Num::Val(v) => lv(v),
            _ => None,
        };
    }
    if let Some(r) = w.strip_suffix("-level") {
        return ["first", "second", "third", "fourth", "fifth", "sixth"]
            .iter()
            .position(|x| *x == r)
            .map(|p| p + 1);
    }
    let b = w.as_bytes();
    (b.len() == 2 && b[0] == b'h' && (b'1'..=b'6').contains(&b[1])).then(|| (b[1] - b'0') as usize)
}

/// The consecutive quoted items a region STARTS with, separated only by
/// commas, `and` or `or`; the byte offset just after the last one is returned
/// too. Empty when the region does not start with a quote.
fn leading_quoted_items(region: &str) -> (Vec<String>, usize) {
    let mut out = Vec::new();
    let mut at = 0;
    loop {
        let rest = &region[at..];
        let t = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        let t_lc = t.to_ascii_lowercase();
        let skip = if !out.is_empty() {
            ["and ", "or "]
                .iter()
                .find(|p| t_lc.starts_with(**p))
                .map_or(0, |p| p.len())
        } else {
            0
        };
        let t2 = t[skip..].trim_start();
        let Some(open) = t2.chars().next().filter(|c| matches!(c, '"' | '\u{201C}')) else {
            break;
        };
        let body = &t2[open.len_utf8()..];
        let Some(close) = body.find(['"', '\u{201D}']) else {
            break;
        };
        let item = body[..close].trim();
        if !item.is_empty() {
            out.push(item.to_string());
        }
        at = region.len() - body.len() + close + 1;
    }
    (out, at)
}

/// Comma separated items; the LAST comma item is also split at its last `and`/`or`.
fn split_list(region: &str) -> Vec<String> {
    let strip = |s: &str| -> String {
        let t = s.trim();
        let l = t.to_ascii_lowercase();
        for p in ["and ", "or "] {
            if l.starts_with(p) {
                return t[p.len()..].trim().to_string();
            }
        }
        t.to_string()
    };
    let mut segs: Vec<String> = region
        .split(',')
        .map(strip)
        .filter(|s| !s.is_empty())
        .collect();
    if let Some(last) = segs.pop() {
        let l = last.to_ascii_lowercase();
        let at = [" and ", " or "]
            .iter()
            .filter_map(|m| l.rfind(m).map(|i| (i, m.len())))
            .max_by_key(|x| x.0);
        match at {
            Some((i, n)) if !last[..i].trim().is_empty() && !last[i + n..].trim().is_empty() => {
                segs.push(last[..i].trim().to_string());
                segs.push(last[i + n..].trim().to_string());
            }
            _ => segs.push(last),
        }
    }
    segs
}

/// A list region cut after the item that follows its closing "and" or "or":
/// "interest, deposit, withdraw and balance, and the file needs" is
/// "interest, deposit, withdraw and balance". None when that last item holds
/// another "and" or "or" ("labelling and storage and disposal").
fn closed_list(region: &str) -> Option<&str> {
    let low = region.to_ascii_lowercase();
    let Some(join) = [" and ", " or "].iter().filter_map(|j| low.find(j)).min() else {
        return Some(region);
    };
    let end = region[join..].find(',').map_or(region.len(), |c| join + c);
    let last = &low[join + 4..end];
    (!last.contains(" and ") && !last.contains(" or ")).then_some(&region[..end])
}

/// `(position, forms)` for each "<form> or <form> for the first|second|...|
/// last [one|topic]" in a sentence that speaks of a word "form", for a list of
/// `len` topics.
fn positional_forms(goal: &str, len: usize) -> Vec<(usize, Vec<String>)> {
    const ORDINALS: [&str; 12] = [
        "first", "second", "third", "fourth", "fifth", "sixth", "seventh", "eighth", "ninth",
        "tenth", "eleventh", "twelfth",
    ];
    let mut out = Vec::new();
    for sentence in goal.split_inclusive(['.', '!', '?']) {
        let low = sentence.to_ascii_lowercase();
        if !low
            .split(|c: char| !c.is_ascii_alphabetic())
            .any(|w| matches!(w, "form" | "forms"))
        {
            continue;
        }
        let Some(cue) = ["for example", "for instance", "such as", "e.g."]
            .iter()
            .filter_map(|c| low.find(c).map(|i| i + c.len()))
            .min()
        else {
            continue;
        };
        for seg in low[cue..].split([',', ';']) {
            let seg = seg.trim().trim_end_matches(['.', '!', '?']);
            let seg = seg.strip_prefix("and ").unwrap_or(seg);
            let Some((forms, pos)) = seg.split_once(" for the ") else {
                continue;
            };
            let ord = pos.split_whitespace().next().unwrap_or("");
            let at = match ORDINALS.iter().position(|o| *o == ord) {
                Some(i) if i < len => i,
                None if ord == "last" => len - 1,
                _ => continue,
            };
            let forms: Vec<String> = forms
                .split(" or ")
                .flat_map(|f| f.split(" and "))
                .map(|f| f.trim().to_string())
                .filter(|f| !f.is_empty() && f.split_whitespace().count() == 1)
                .collect();
            if !forms.is_empty() {
                out.push((at, forms));
            }
        }
    }
    out
}

fn trim_title(t: &str) -> String {
    t.trim()
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '\u{201C}' | '\u{201D}' | '*' | '_' | '`'))
        .trim()
        .to_string()
}

/// The requirements stated in `goal` and the spans that could not be read.
pub fn analyze(goal: &str) -> Extraction {
    let toks = tokenize(goal);
    let n = toks.len();
    let mut handled: std::collections::BTreeSet<usize> = Default::default();
    let mut unsure: Vec<(usize, String)> = Vec::new();
    let mut counts: Vec<Requirement> = Vec::new();
    let mut headings: Vec<String> = Vec::new();
    let mut level_headings: Vec<(usize, Vec<String>)> = Vec::new();
    let mut topics: Vec<String> = Vec::new();
    let mut phrases: Vec<String> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let add = |v: &mut Vec<Requirement>, r: Requirement| {
        if !v.contains(&r) {
            v.push(r);
        }
    };
    let bad = |unsure: &mut Vec<(usize, String)>, at: usize, commas: bool| {
        unsure.push((at, snippet(goal, &toks, at, commas)));
    };

    // ---- counts: cue N noun, and N or more|fewer noun, N+ noun ----
    let count_req = |dir: Dir, noun: &str, v: usize| -> Option<Requirement> {
        Some(match (dir, noun) {
            (Dir::Min, "lines" | "line") => Requirement::MinLines(v),
            (Dir::Max, "lines" | "line") => Requirement::MaxLines(v),
            (Dir::Min, "words" | "word") => Requirement::MinWords(v),
            (Dir::Max, "words" | "word") => Requirement::MaxWords(v),
            (Dir::Min, "items" | "item") => Requirement::MinItems(ItemKind::Items, v),
            (Dir::Min, "steps" | "step") => Requirement::MinItems(ItemKind::Steps, v),
            (Dir::Min, "sections" | "section" | "headings" | "heading") => {
                Requirement::MinItems(ItemKind::Sections, v)
            }
            (Dir::Min, "questions" | "question") => Requirement::MinItems(ItemKind::Questions, v),
            _ => return None,
        })
    };
    for s in 0..n {
        // "N <noun> or more|fewer|less" and "N <noun> minimum|maximum".
        if let (Num::Val(v), Some(noun)) = (
            parse_num(&toks[s].word),
            toks.get(s + 1).map(|t| t.word.as_str()),
        ) {
            let w = |i: usize| toks.get(i).map(|t| t.word.as_str());
            let post = match (w(s + 2), w(s + 3)) {
                (Some("or"), Some("more")) => Some((Dir::Min, s + 3)),
                (Some("or"), Some("fewer" | "less")) => Some((Dir::Max, s + 3)),
                (Some("minimum" | "min"), _) => Some((Dir::Min, s + 2)),
                (Some("maximum" | "max"), _) => Some((Dir::Max, s + 2)),
                _ => None,
            };
            if let Some((dir, end)) = post {
                if NOUNS.contains(&noun) && !negated_before(&toks, s) && qualifier_ok(&toks, end) {
                    if let Some(r) = count_req(dir, noun, v) {
                        add(&mut counts, r);
                        handled.extend(s..=end);
                        continue;
                    }
                }
            }
        }
        // "N or more|fewer|less <noun>" and "N+ <noun>": the number leads.
        let lead = match parse_num(toks[s].word.trim_end_matches('+')) {
            Num::Val(v) if toks[s].word.ends_with('+') => Some((v, Dir::Min, s + 1)),
            Num::Val(v)
                if toks.get(s + 1).is_some_and(|t| t.word == "or")
                    && toks
                        .get(s + 2)
                        .is_some_and(|t| matches!(t.word.as_str(), "more" | "fewer" | "less")) =>
            {
                let d = if toks[s + 2].word == "more" {
                    Dir::Min
                } else {
                    Dir::Max
                };
                Some((v, d, s + 3))
            }
            _ => None,
        };
        if let Some((v, dir, j)) = lead {
            if let Some(noun) = toks.get(j).map(|t| t.word.as_str()) {
                if NOUNS.contains(&noun) && !negated_before(&toks, s) && qualifier_ok(&toks, j) {
                    if let Some(r) = count_req(dir, noun, v) {
                        add(&mut counts, r);
                        handled.extend(s..=j);
                    }
                }
            }
            continue;
        }
        let Some((len, dir, adj)) = match_cue(&toks, s, false) else {
            continue;
        };
        let i = s + len;
        let Some(numtok) = toks.get(i) else { continue };
        let Num::Val(v) = parse_num(&numtok.word) else {
            continue;
        };
        let j = i + 1;
        let Some(noun) = toks.get(j).map(|t| t.word.as_str()) else {
            continue;
        };
        if !NOUNS.contains(&noun) || negated_before(&toks, s) {
            continue;
        }
        if matches!(noun, "sentences" | "sentence") {
            let after = per_section_after(&toks, j);
            if dir == Dir::Min && (after.is_some() || every_section_before(&toks, s)) {
                let v = (v as i64 + adj) as usize;
                add(&mut counts, Requirement::MinSentencesPerSection(v));
                handled.extend(s..=after.unwrap_or(j));
            }
            continue;
        }
        if matches!(noun, "words" | "word") {
            if let Some(end) = plain_text_after(&toks, j) {
                // "N words of plain text" is a count over a scope the goal must name.
                if dir == Dir::Min {
                    if let Some(scope) = section_scope_before(goal, &toks, s) {
                        let v = v as i64 + adj;
                        if v >= 1 {
                            add(
                                &mut counts,
                                Requirement::SectionMinWords {
                                    title: scope,
                                    n: v as usize,
                                },
                            );
                            handled.extend(s..=end);
                        }
                    }
                }
                continue;
            }
        }
        if !qualifier_ok(&toks, j) {
            continue;
        }
        let v = v as i64 + adj;
        if v < 1 {
            continue;
        }
        if let Some(r) = count_req(dir, noun, v as usize) {
            add(&mut counts, r);
            handled.extend(s..=j);
        }
    }

    // ---- bare line counts: "Add 2 lines", "Write 20 lines about X", "a two-line poem" ----
    // The verb decides the reading: add|append|insert|with N lines = exactly N
    // lines added; write|put|create|draft|... N lines = at least N lines added;
    // an adjective "N-line" after a verb = exactly N lines. Measured against the
    // prior file (a new file has none), see `Requirement::AddedLines`.
    for s in 0..n {
        let Some(mut exact) = line_verb(&toks[s].word) else {
            continue;
        };
        if handled.contains(&s) || negated_before(&toks, s) {
            continue;
        }
        let mut i = s + 1;
        while toks.get(i).is_some_and(|t| {
            matches!(
                t.word.as_str(),
                "a" | "an" | "the" | "just" | "only" | "exactly"
            )
        }) {
            if toks[i].word == "exactly" {
                exact = true;
            }
            i += 1;
        }
        let Some(t) = toks.get(i) else { continue };
        if handled.contains(&i) {
            continue;
        }
        // "two-line", "1-line" (an adjective: the document is that long)
        if let Some((a, b)) = t.word.split_once('-') {
            if let (Num::Val(v), "line" | "lines") = (parse_num(a), b) {
                add(
                    &mut counts,
                    Requirement::AddedLines {
                        n: v,
                        exact: true,
                        prior: None,
                    },
                );
                handled.extend(s..=i);
            }
            continue;
        }
        let num = if t.word == "single" {
            Num::Val(1)
        } else {
            parse_num(&t.word)
        };
        let (Num::Val(v), Some(noun)) = (num, toks.get(i + 1).map(|t| t.word.as_str())) else {
            continue;
        };
        if !matches!(noun, "line" | "lines") || handled.contains(&(i + 1)) {
            continue;
        }
        // "N lines at least" is a lower bound; "at most" and the like stay uncertain.
        let at = toks.get(i + 2).map(|t| t.word.as_str());
        let bound = at == Some("at") && toks.get(i + 3).is_some_and(|t| t.word == "least");
        let loose = at == Some("at")
            && toks
                .get(i + 3)
                .is_some_and(|t| matches!(t.word.as_str(), "most" | "max" | "the"));
        let tail_ok = !loose
            && (qualifier_ok(&toks, i + 1)
                || toks
                    .get(i + 2)
                    .is_some_and(|t| LINE_TAILS.contains(&t.word.as_str())));
        if tail_ok {
            add(
                &mut counts,
                Requirement::AddedLines {
                    n: v,
                    exact: exact && !bound,
                    prior: None,
                },
            );
            handled.extend(s..=i + 1 + if bound { 2 } else { 0 });
        }
    }

    // ---- fenced code blocks: "include three separate fenced code examples" ----
    for i in 0..n {
        let Num::Val(v) = parse_num(&toks[i].word) else {
            continue;
        };
        if handled.contains(&i) {
            continue;
        }
        let Some(j) = (i + 1..=i + 4).find(|&j| {
            toks.get(j)
                .is_some_and(|t| CODE_NOUNS.contains(&t.word.as_str()))
        }) else {
            continue;
        };
        let between = &toks[i + 1..j];
        if !between.iter().all(|t| CODE_ADJ.contains(&t.word.as_str()))
            || !between
                .iter()
                .any(|t| matches!(t.word.as_str(), "fenced" | "code"))
        {
            continue;
        }
        let tail_ok = qualifier_ok(&toks, j)
            || toks.get(j + 1).is_some_and(|t| {
                matches!(
                    t.word.as_str(),
                    "introduced" | "showing" | "demonstrating" | "illustrating"
                )
            });
        if !tail_ok {
            continue;
        }
        // The direction: a cue ending right before the number, else a plain
        // "include|add|put|provide ... N" (at least N).
        let mut dir_adj: Option<(usize, Dir, i64)> = None;
        for len in 1..=3usize {
            if i >= len {
                if let Some((l, d, a)) = match_cue(&toks, i - len, false) {
                    if l == len {
                        dir_adj = Some((len, d, a));
                        break;
                    }
                }
            }
        }
        let start = match dir_adj {
            Some((len, _, _)) => i - len,
            None => {
                let mut k = i;
                while k > 0 && matches!(toks[k - 1].word.as_str(), "a" | "an" | "the" | "separate")
                {
                    k -= 1;
                }
                if k > 0 && CODE_VERBS.contains(&toks[k - 1].word.as_str()) {
                    i
                } else {
                    continue;
                }
            }
        };
        let (dir, adj) = dir_adj.map_or((Dir::Min, 0), |(_, d, a)| (d, a));
        let v = v as i64 + adj;
        if dir == Dir::Min && v >= 1 && !negated_before(&toks, start) {
            add(
                &mut counts,
                Requirement::MinItems(ItemKind::CodeBlocks, v as usize),
            );
            handled.extend(start..=j);
        }
    }

    // ---- section titles: "sections titled A, B and C", "sections: A, B and C" ----
    for k in 0..n {
        let w = toks[k].word.as_str();
        let titled = matches!(w, "titled" | "named" | "called" | "entitled")
            && k >= 1
            && SECTION_NOUNS.contains(&toks[k - 1].word.as_str());
        let colon = SECTION_NOUNS.contains(&w) && toks[k].raw.ends_with(':');
        if !titled && !colon {
            continue;
        }
        let noun_at = if titled { k - 1 } else { k };
        let declared_at = [1usize, 2].into_iter().find_map(|d| {
            let p = noun_at.checked_sub(d)?;
            matches!(parse_num(&toks[p].word), Num::Val(_)).then_some(p)
        });
        let declared = declared_at.and_then(|p| match parse_num(&toks[p].word) {
            Num::Val(v) => Some(v),
            _ => None,
        });
        let region = list_region(goal, toks[k].end);
        let (quotes, quotes_end) = leading_quoted_items(region);
        let level = (titled && noun_at >= 1)
            .then(|| level_adj(&toks[noun_at - 1].word))
            .flatten();
        let quoted_list = !quotes.is_empty();
        let titles: Vec<String> = if quotes.is_empty() {
            split_list(region).iter().map(|t| trim_title(t)).collect()
        } else {
            quotes
        };
        let ok = !negated_before(&toks, noun_at)
            && !titles.is_empty()
            && titles.len() <= 40
            && titles
                .iter()
                .all(|t| !t.is_empty() && t.split_whitespace().count() <= 8)
            && (quoted_list || !titles.iter().any(|t| has_count_and_noun(t)))
            && declared.is_none_or(|d| d == titles.len());
        if ok {
            let seen = match level {
                Some(l) => {
                    let at = level_headings.iter().position(|x| x.0 == l);
                    let slot = match at {
                        Some(p) => &mut level_headings[p].1,
                        None => {
                            level_headings.push((l, Vec::new()));
                            &mut level_headings.last_mut().expect("just pushed").1
                        }
                    };
                    slot
                }
                None => &mut headings,
            };
            for t in titles {
                if !seen.contains(&t) {
                    seen.push(t);
                }
            }
            let from = declared_at.unwrap_or(if level.is_some() {
                noun_at - 1
            } else {
                noun_at
            });
            handled.extend(from..=k);
            // Quoted titles consume only the quoted list; text after it is read on its own.
            let upto = if quotes_end > 0 {
                quotes_end
            } else {
                region.len()
            };
            mark_bytes(&mut handled, &toks, toks[k].end, toks[k].end + upto);
        } else {
            bad(&mut unsure, noun_at.min(k), false);
        }
    }

    // ---- topics: "covers A, B and C" ----
    // The topics of the last counted list, for the forms a later sentence ties
    // to them by position ("withdrew ... for the third").
    let mut counted_topics: Vec<usize> = Vec::new();
    for k in 0..n {
        let w = toks[k].word.as_str();
        let starts_sentence = k == 0 || toks[k - 1].raw.ends_with(['.', '!', '?']);
        let key = matches!(w, "covers" | "covering")
            || (w == "cover"
                && k >= 1
                && matches!(
                    toks[k - 1].word.as_str(),
                    "to" | "should" | "must" | "will" | "can" | "also" | "shall" | "would" | "and"
                ));
        // "cover|covers|covering N topics: a, b and c" (sc#332): the count must
        // equal the list, which ends with the item after its "and" or "or".
        let counted = matches!(w, "cover" | "covers" | "covering")
            && (key || starts_sentence)
            && matches!(toks.get(k + 2), Some(t) if matches!(t.word.as_str(), "topics" | "topic") && t.raw.ends_with(':'));
        if counted {
            let declared = match parse_num(&toks[k + 1].word) {
                Num::Val(v) => Some(v),
                _ => None,
            };
            let region = closed_list(list_region(goal, toks[k + 2].end)).unwrap_or("");
            let items: Vec<String> = split_list(region).iter().map(|t| trim_title(t)).collect();
            let ok = !region.is_empty()
                && declared == Some(items.len())
                && !negated_before(&toks, k)
                && items.len() <= 12
                && !items.iter().any(|t| has_count_and_noun(t))
                && items
                    .iter()
                    .all(|t| (1..=4).contains(&content_words(t).len()));
            if ok {
                counted_topics.clear();
                for t in items {
                    let at = topics.iter().position(|x| *x == t).unwrap_or_else(|| {
                        topics.push(t);
                        topics.len() - 1
                    });
                    counted_topics.push(at);
                }
                handled.extend(k..=k + 2);
                mark_bytes(
                    &mut handled,
                    &toks,
                    toks[k + 2].end,
                    toks[k + 2].end + region.len(),
                );
            } else {
                bad(&mut unsure, k, false);
            }
            continue;
        }
        if !key {
            continue;
        }
        let region = list_region(goal, toks[k].end);
        let items = split_list(region);
        let ok = !negated_before(&toks, k)
            && !items.iter().any(|t| has_count_and_noun(t))
            && !items.is_empty()
            && items.len() <= 12
            && items.iter().all(|t| {
                let c = content_words(t).len();
                (1..=4).contains(&c)
            });
        if ok {
            for t in items {
                let t = trim_title(&t);
                if !topics.contains(&t) {
                    topics.push(t);
                }
            }
            handled.insert(k);
            mark_bytes(&mut handled, &toks, toks[k].end, toks[k].end + region.len());
        } else {
            bad(&mut unsure, k, false);
        }
    }

    // Forms a goal ties to a counted topic by position (sc#332): "for example
    // interests or interested for the first one, ..., withdrew or withdrawing
    // for the third". A form the word-form rule already accepts is not added.
    if !counted_topics.is_empty() {
        for (at, forms) in positional_forms(goal, counted_topics.len()) {
            let i = counted_topics[at];
            let (topic, _) = topic_forms(&topics[i]);
            let words = content_words(topic);
            // Only a form of this topic: one word, the topic one word, the same
            // first three letters, and not already matched by the word-form rule.
            let [word] = words.as_slice() else {
                continue;
            };
            let keys = word_keys(word, true);
            let extra: Vec<String> = forms
                .into_iter()
                .filter(|f| {
                    f.get(..3).is_some_and(|p| word.starts_with(p))
                        && !word_keys(f, false).iter().any(|k| keys.contains(k))
                })
                .collect();
            if !extra.is_empty() {
                topics[i] = format!("{topic} (or {})", extra.join(", "));
            }
        }
    }

    // ---- required words and phrases ----
    // `include|contain the phrase "X"` (one quoted phrase, ending its clause).
    let low = goal.to_ascii_lowercase();
    for key in ["include the phrase \"", "contain the phrase \""] {
        let mut from = 0;
        while let Some(i) = low[from..].find(key) {
            let key_at = from + i;
            let start = key_at + key.len();
            let Some(len) = low[start..].find('"') else {
                break;
            };
            let end = start + len;
            let before_n = toks.iter().take_while(|t| t.start < key_at).count();
            let negated = negated_before(&toks, before_n);
            let after = low[end + 1..].split_whitespace().next();
            let closes_clause = low[end + 1..].starts_with([',', '.', ';', ':', '!', '?', ')'])
                || after.is_none_or(|nx| CLAUSE_JOINERS.contains(&clean(nx)));
            if len > 0
                && !negated
                && closes_clause
                && goal.is_char_boundary(start)
                && goal.is_char_boundary(end)
            {
                let p = goal[start..end].to_string();
                if !phrases.contains(&p) {
                    phrases.push(p);
                }
                if let Some(t) = toks.iter().position(|t| t.start == key_at) {
                    handled.insert(t);
                }
                mark_bytes(&mut handled, &toks, key_at, end + 1);
            }
            from = end + 1;
        }
    }
    // Any other `include|contain|use|mention the word(s)|term(s)|phrase(s) ...`.
    for k in 0..n {
        if !matches!(
            toks[k].word.as_str(),
            "include"
                | "includes"
                | "contain"
                | "contains"
                | "use"
                | "uses"
                | "mention"
                | "mentions"
        ) || toks.get(k + 1).map(|t| t.word.as_str()) != Some("the")
        {
            continue;
        }
        let Some(kind) = toks.get(k + 2).map(|t| t.word.as_str()) else {
            continue;
        };
        if !matches!(
            kind,
            "word" | "words" | "term" | "terms" | "phrase" | "phrases"
        ) {
            continue;
        }
        if handled.contains(&k) {
            continue;
        }
        if kind == "phrase" {
            // The singular quoted form is the block above; anything it refused stays uncertain.
            bad(&mut unsure, k, false);
            continue;
        }
        let region = list_region(goal, toks[k + 2].end);
        let quotes = quoted_items(region);
        let (items, tail_ok) = if quotes.is_empty() {
            let items: Vec<String> = split_list(region).iter().map(|t| trim_title(t)).collect();
            let single = items
                .iter()
                .all(|t| !t.is_empty() && t.split_whitespace().count() == 1);
            (items, single && kind != "phrases")
        } else {
            let last_q = region.rfind(['"', '\u{201D}']).unwrap_or(0);
            let tail = region[last_q + 1..].trim().to_ascii_lowercase();
            let tail_word = tail.split_whitespace().next().map(clean).unwrap_or("");
            (
                quotes,
                tail.is_empty() || CLAUSE_JOINERS.contains(&tail_word),
            )
        };
        if !negated_before(&toks, k) && tail_ok && !items.is_empty() && items.len() <= 20 {
            let dst = if kind == "phrases" {
                &mut phrases
            } else {
                &mut words
            };
            for t in items {
                if !dst.contains(&t) {
                    dst.push(t);
                }
            }
            handled.extend(k..=k + 2);
            mark_bytes(
                &mut handled,
                &toks,
                toks[k + 2].end,
                toks[k + 2].end + region.len(),
            );
        } else {
            bad(&mut unsure, k, false);
        }
    }
    // The singular phrase key that matched no quoted text at all.
    for k in 0..n {
        if toks[k].word.starts_with("includ") || toks[k].word.starts_with("contain") {
            let w2 = |d: usize| toks.get(k + d).map(|t| t.word.as_str());
            if w2(1) == Some("the") && w2(2) == Some("phrase") && !handled.contains(&k) {
                bad(&mut unsure, k, false);
            }
        }
    }

    // ---- first-line heading: "start with a [markdown] [level-N] heading [line]
    //      [that begins with "# "]" ----
    let mut first_heading: Option<Option<usize>> = None;
    for v in 0..n {
        let w = |i: usize| toks.get(i).map(|t| t.word.as_str());
        if handled.contains(&v)
            || !matches!(
                w(v),
                Some("start" | "starts" | "begin" | "begins" | "open" | "opens")
            )
            || w(v + 1) != Some("with")
            || w(v + 2) != Some("a")
            || (v..v + 2).any(|i| ends_clause(&toks[i].raw))
        {
            continue;
        }
        let mut k = v + 3;
        if w(k) == Some("markdown") && !ends_clause(&toks[k].raw) {
            k += 1;
        }
        let adj = w(k).and_then(level_adj);
        if adj.is_some() && !ends_clause(&toks[k].raw) {
            k += 1;
        }
        if w(k) != Some("heading") {
            // "start with a **heading**", "start with a short heading": a heading
            // wording this recognizer does not read is uncertain, never silent.
            if (v + 3..=v + 5).any(|i| toks.get(i).is_some_and(|t| t.norm.starts_with("heading"))) {
                bad(&mut unsure, v, false);
            }
            continue;
        }
        if w(k + 1) == Some("line") && !ends_clause(&toks[k].raw) {
            k += 1;
        }
        if negated_before(&toks, v) {
            bad(&mut unsure, v, false);
            continue;
        }
        // The heading clause must END its sentence: anything after it in the same
        // sentence ("and keep it brief", ", then a table") is left to the refusal below,
        // so no sibling constraint is swallowed unchecked.
        let sentence_end = |raw: &str| raw.ends_with(['.', '!', '?']);
        let found = if sentence_end(&toks[k].raw) || k + 1 == n {
            Some((adj, k))
        } else if matches!(w(k + 1), Some("that" | "which"))
            && matches!(w(k + 2), Some("begins" | "starts" | "begin" | "start"))
            && w(k + 3) == Some("with")
            && (k..k + 3).all(|i| !ends_clause(&toks[i].raw))
        {
            // The marker: a quoted run of 1 to 6 `#` and one space, then the end of
            // the clause. Anything else ("#" alone, "a hash", a qualifier) is uncertain.
            let from = toks[k + 3].end;
            let rest = &goal[from..];
            let lead = rest.len() - rest.trim_start().len();
            let mut cs = rest[lead..].char_indices();
            let marker = cs.next().and_then(|(_, q)| {
                let close: &[char] = match q {
                    '"' => &['"'],
                    '\u{201C}' => &['\u{201D}', '"'],
                    '\'' => &['\''],
                    '`' => &['`'],
                    _ => return None,
                };
                let body = &rest[lead + q.len_utf8()..];
                let h = body.bytes().take_while(|&b| b == b'#').count();
                let after_space = body[h..].strip_prefix(' ')?;
                let c = after_space.chars().next().filter(|c| close.contains(c))?;
                let end = from + rest.len() - after_space.len() + c.len_utf8();
                let tail_ok =
                    goal[end..].trim_end().is_empty() || goal[end..].starts_with(['.', '!', '?']);
                ((1..=6).contains(&h) && tail_ok).then_some((h, end))
            });
            match marker {
                Some((h, end)) if adj.is_none_or(|a| a == h) => {
                    mark_bytes(&mut handled, &toks, from, end);
                    let last = (0..n).rev().find(|&i| toks[i].start < end).unwrap_or(k + 3);
                    Some((Some(h), last))
                }
                _ => None,
            }
        } else {
            None
        };
        match found {
            Some((level, last)) if first_heading.is_none_or(|f| f == level) => {
                first_heading = Some(level);
                handled.extend(v..=last.max(k));
            }
            _ => bad(&mut unsure, v, false),
        }
    }

    // ---- candidates the recognizers above did not claim ----
    for s in 0..n {
        if handled.contains(&s) {
            continue;
        }
        // cue N <noun> (an adjective may sit between), in the emphasis-stripped view
        let after = |i: usize| toks.get(i).map(|t| t.norm.as_str());
        let cue = match_cue(&toks, s, true)
            .map(|c| c.0)
            .or_else(|| VAGUE.contains(&toks[s].norm.as_str()).then_some(1));
        if let Some(len) = cue {
            let vague = len == 1 && VAGUE.contains(&toks[s].norm.as_str());
            let i = s + len;
            let reach = if vague { 4 } else { 2 };
            let num_at = after(i).map(parse_num);
            let looks_numeric = matches!(num_at, Some(Num::Val(_)) | Some(Num::Bad));
            let noun_near =
                (i + 1..=i + reach).any(|j| after(j).is_some_and(|w| NOUNS.contains(&w)));
            if looks_numeric && (noun_near || num_at == Some(Num::Bad)) {
                bad(&mut unsure, s, true);
                continue;
            }
        }
        // "20 or more lines" / "20+ lines" in emphasis, and "35-line"
        let w = toks[s].norm.as_str();
        if let Some((a, b)) = w.split_once('-') {
            let noun = b.trim_end_matches('s');
            if matches!(parse_num(a), Num::Val(_))
                && (NOUNS.contains(&format!("{noun}s").as_str()) || NOUNS.contains(&noun))
            {
                bad(&mut unsure, s, true);
                continue;
            }
        }
        let trailing_plus =
            w.ends_with('+') && matches!(parse_num(w.trim_end_matches('+')), Num::Val(_));
        let or_more = matches!(parse_num(w), Num::Val(_))
            && after(s + 1) == Some("or")
            && matches!(after(s + 2), Some("more" | "fewer" | "less"));
        if (trailing_plus || or_more)
            && (s + 1..=s + 3).any(|j| after(j).is_some_and(|x| NOUNS.contains(&x)))
        {
            bad(&mut unsure, s, true);
        }
    }

    // ---- safety net: a countable noun with a number, or a quantity or list cue ----
    // Anything the recognizers above did not consume is UNCERTAIN, so a
    // wording nobody listed is reported instead of silently passing.
    let nm = |i: usize| toks.get(i).map(|t| t.norm.as_str());
    for (i, tk) in toks.iter().enumerate() {
        let w = tk.norm.as_str();
        // N [adjective] [adjective] <noun>
        if let Num::Val(_) = parse_num(w) {
            let next = nm(i + 1).unwrap_or("");
            let pronoun = w == "one"
                && matches!(
                    next,
                    "of" | "can"
                        | "may"
                        | "should"
                        | "must"
                        | "that"
                        | "who"
                        | "which"
                        | "is"
                        | "to"
                        | "more"
                        | "another"
                        | "thing"
                        | "day"
                        | "time"
                );
            // "twelve of them": a count of items the goal does not name.
            if w != "one"
                && nm(i + 1) == Some("of")
                && matches!(nm(i + 2), Some("them" | "these" | "those"))
                && !handled.contains(&i)
            {
                bad(&mut unsure, i, true);
            }
            if !pronoun {
                // The window never crosses the end of a sentence.
                let stop = (i..i + 4)
                    .find(|&k| {
                        toks.get(k)
                            .is_some_and(|t| t.raw.ends_with(['.', '!', '?']))
                    })
                    .map_or(i + 4, |k| k.max(i + 1));
                if let Some(j) = (i + 1..=stop).find(|&j| nm(j).is_some_and(|x| NOUNS.contains(&x)))
                {
                    if !handled.contains(&i) || !handled.contains(&j) {
                        bad(&mut unsure, i, true);
                    }
                }
            }
        }
        // "2-3 lines" (a range), "three-plus sections", "a dozen lines"
        if !handled.contains(&i) {
            let noun_next = (i + 1..=i + 2).any(|j| nm(j).is_some_and(|x| NOUNS.contains(&x)));
            let range = w.split_once('-').is_some_and(|(a, b)| {
                matches!(parse_num(a), Num::Val(_))
                    && (matches!(parse_num(b), Num::Val(_)) || b == "plus")
            });
            if (range || matches!(w, "dozen" | "dozens")) && noun_next {
                bad(&mut unsure, i, true);
            }
            // "line count at least 40", "word count of 200"
            if w == "count"
                && i >= 1
                && nm(i - 1).is_some_and(|x| NOUNS.contains(&x) || matches!(x, "line" | "word"))
                && (i + 1..=i + 6).any(|j| matches!(nm(j).map(parse_num), Some(Num::Val(_))))
            {
                bad(&mut unsure, i - 1, true);
            }
            // a heading level adjective that no title list consumed
            if level_adj(w).is_some() && nm(i + 1).is_some_and(|x| SECTION_NOUNS.contains(&x)) {
                bad(&mut unsure, i, true);
            }
        }
        // quantity cues next to a countable noun: twice, minimum, "or more", ...
        let cue = matches!(
            w,
            "twice" | "thrice" | "minimum" | "maximum" | "limit" | "min" | "max"
        ) || (w == "or" && matches!(nm(i + 1), Some("more" | "fewer" | "less")))
            || (matches!(w, "longer" | "shorter") && nm(i + 1) == Some("than"));
        if cue && !handled.contains(&i) {
            let lo = i.saturating_sub(3);
            if (lo..=i + 3).any(|j| nm(j).is_some_and(|x| NOUNS.contains(&x))) {
                bad(&mut unsure, i.saturating_sub(1), true);
            }
        }
        // heading and word lists without a number
        let heading_list = matches!(w, "heading" | "headings")
            && (nm(i + 1) == Some("for")
                || tk.raw.ends_with(':')
                || (i >= 2
                    && matches!(nm(i - 2), Some("a" | "the"))
                    && i >= 3
                    && matches!(
                        nm(i - 3),
                        Some(
                            "needs"
                                | "need"
                                | "must"
                                | "should"
                                | "have"
                                | "has"
                                | "include"
                                | "includes"
                                | "contain"
                                | "contains"
                                | "with"
                                | "and"
                        )
                    )));
        let word_list = matches!(w, "word" | "words")
            && (tk.raw.ends_with(':')
                || (i >= 1
                    && matches!(
                        nm(i - 1),
                        Some("these" | "following" | "those" | "the" | "key")
                    )));
        if (heading_list || word_list) && !handled.contains(&i) {
            bad(&mut unsure, i.saturating_sub(2), false);
        }
    }

    let mut requirements = counts;
    if !headings.is_empty() {
        requirements.push(Requirement::RequiredHeadings(headings));
    }
    for (level, titles) in level_headings {
        requirements.push(Requirement::LevelHeadings { level, titles });
    }
    if !topics.is_empty() {
        requirements.push(Requirement::RequiredTopics(topics));
    }
    if !phrases.is_empty() {
        requirements.push(Requirement::RequiredPhrases(phrases));
    }
    if !words.is_empty() {
        requirements.push(Requirement::RequiredWords(words));
    }
    if let Some(level) = first_heading {
        requirements.push(Requirement::FirstLineHeading { level });
    }
    unsure.sort_by_key(|u| u.0);
    let mut uncertain: Vec<String> = Vec::new();
    let all: Vec<String> = unsure.into_iter().map(|u| u.1).collect();
    for t in &all {
        // A span inside a longer reported span is the same finding.
        let inside = all
            .iter()
            .any(|o| o.len() > t.len() && o.contains(t.as_str()));
        if !inside && !uncertain.contains(t) {
            uncertain.push(t.clone());
        }
    }
    Extraction {
        requirements,
        uncertain,
    }
}
