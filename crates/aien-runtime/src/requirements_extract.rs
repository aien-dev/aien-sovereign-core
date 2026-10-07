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
//! | `<noun>` = `lines`, `words`; `items`, `steps`, `sections`, `headings`, `questions` (Min only; singular too) | lines, words, list items, numbered steps, headings, questions |
//! | `N sentences in|per|for|within every|each section` (or `in every section, ... N sentences`) | `MinSentencesPerSection(N)` |
//! | `sections|headings titled|named|called A, B and C`, `sections: A, B and C` | `RequiredHeadings` |
//! | `covers|covering A, B and C` (and `to|should|must|will|can cover ...`) | `RequiredTopics` |
//! | `include|contain|use|mention the word(s)|term(s) X, Y` or `"X", "Y"` | `RequiredWords` |
//! | `include|contain|use|mention the phrase "X"` / `the phrases "X", "Y"` | `RequiredPhrases` |
//!
//! `N` is digits or a number word (`one` to `twenty`, tens up to `hundred`,
//! `twenty-five`), at least 1.
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
//!   `between 10 and 20 lines`, `35-line`) is UNCERTAIN.
//! - a title list whose parsed length differs from a declared count ("6
//!   sections titled A, B, C") is UNCERTAIN; so is a list item that is not a
//!   short noun phrase.
//! - the title or topic list is the text after the key word up to the end of
//!   the sentence (or `, each|with|which|that|...`, or a count bound); a
//!   trailing `and` splits the LAST comma item (`Who to ask and Glossary`),
//!   so a title that itself contains `and` is only safe inside the list.

use crate::requirements::{content_words, ItemKind, Requirement};

/// What a goal says: recognized requirements and the spans it could not read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Extraction {
    pub requirements: Vec<Requirement>,
    /// Goal text that looks like an explicit measurable requirement but could
    /// not be interpreted reliably. Non-empty = the task must be refused.
    pub uncertain: Vec<String>,
}

impl Extraction {
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
const NOUN_FOLLOWERS: [&str; 9] = [
    "long", "with", "total", "overall", "titled", "named", "called", "about", "on",
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
const CUES: [(&[&str], Dir, i64); 20] = [
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

const NOUNS: [&str; 34] = [
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
            CLAUSE_JOINERS.contains(&w)
                || NOUN_FOLLOWERS.contains(&w)
                || NOUN_FOLLOWERS_2.contains(&w)
                || (w == "in" && toks.get(j + 2).is_some_and(|t| clean(&t.raw) == "total"))
        }
    }
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
fn per_section_after(toks: &[Tok], j: usize) -> bool {
    let w = |i: usize| toks.get(i).map(|t| t.word.as_str());
    if !matches!(w(j + 1), Some("in" | "per" | "for" | "within" | "under")) {
        return false;
    }
    let mut k = j + 2;
    if matches!(w(k), Some("every" | "each" | "a" | "the" | "any")) {
        k += 1;
    }
    matches!(w(k), Some("section" | "sections" | "heading" | "headings"))
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
            if dir == Dir::Min && (per_section_after(&toks, j) || every_section_before(&toks, s)) {
                let v = (v as i64 + adj) as usize;
                add(&mut counts, Requirement::MinSentencesPerSection(v));
                handled.extend(s..=j);
            }
            continue;
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
        let declared = noun_at
            .checked_sub(1)
            .and_then(|p| match parse_num(&toks[p].word) {
                Num::Val(v) => Some(v),
                _ => None,
            });
        let region = list_region(goal, toks[k].end);
        let quotes = quoted_items(region);
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
            && declared.is_none_or(|d| d == titles.len());
        if ok {
            for t in titles {
                if !headings.contains(&t) {
                    headings.push(t);
                }
            }
            handled.insert(k);
        } else {
            bad(&mut unsure, noun_at.min(k), false);
        }
    }

    // ---- topics: "covers A, B and C" ----
    for k in 0..n {
        let w = toks[k].word.as_str();
        let key = matches!(w, "covers" | "covering")
            || (w == "cover"
                && k >= 1
                && matches!(
                    toks[k - 1].word.as_str(),
                    "to" | "should" | "must" | "will" | "can" | "also" | "shall" | "would" | "and"
                ));
        if !key {
            continue;
        }
        let items = split_list(list_region(goal, toks[k].end));
        let ok = !negated_before(&toks, k)
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
        } else {
            bad(&mut unsure, k, false);
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
            handled.insert(k);
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

    let mut requirements = counts;
    if !headings.is_empty() {
        requirements.push(Requirement::RequiredHeadings(headings));
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
    unsure.sort_by_key(|u| u.0);
    let mut uncertain: Vec<String> = Vec::new();
    for (_, t) in unsure {
        if !uncertain.contains(&t) {
            uncertain.push(t);
        }
    }
    Extraction {
        requirements,
        uncertain,
    }
}
