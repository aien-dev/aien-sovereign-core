//! Requirement validation for composed documents.
//!
//! A task goal can state a measurable requirement ("in at least 20 lines").
//! Before this module nothing checked it: a 13-line answer to such a goal was
//! approved and committed. This module defines a small typed set of
//! requirements, extracts them deterministically from the goal text, and
//! checks the COMPLETE parsed document (the content that would be saved).
//!
//! ## Supported requirements (everything else is NOT extracted)
//!
//! Extraction matches the exact word sequence `at least|at most N <noun>`
//! (case-insensitive, N written in digits, N >= 1, trailing punctuation on a
//! word ignored) and `include|contain the phrase "<text>"` (double quotes).
//! A goal that states a requirement in any other wording is not covered and
//! gets no check; `ComposeTaskReport::requirements_recognized` lists exactly
//! what was recognized.
//!
//! | goal wording                | requirement      | how it is counted |
//! |-----------------------------|------------------|-------------------|
//! | `at least N lines`          | `MinLines(N)`    | non-empty lines (a line with a non-whitespace character) |
//! | `at most N lines`           | `MaxLines(N)`    | non-empty lines |
//! | `at least N items`          | `MinItems(Items, N)` | markdown list items: a line starting (after indentation) with `- `, `* `, `+ ` or `<digits>. ` / `<digits>) ` |
//! | `at least N steps`          | `MinItems(Steps, N)` | numbered list items only: `<digits>. ` / `<digits>) ` |
//! | `at least N sections`       | `MinItems(Sections, N)` | markdown headings: 1 to 6 `#` then a space then text |
//! | `at least N questions`      | `MinItems(Questions, N)` | non-empty lines whose last character is `?` |
//! | `include the phrase "X"`    | `RequiredPhrases([X])` | case-insensitive substring of the document |
//!
//! `item`, `step`, `section`, `question` (singular) are accepted too.
//!
//! ## What a requirement is judged on
//!
//! The COMPLETE document that would be saved (in edit mode the merged file,
//! not the model's reply). An edit only adds lines, so a prior file that
//! already exceeds a `MaxLines` is refused before any attempt.
//!
//! Counting: `MinLines`/`MaxLines` count every non-empty line of the file,
//! INCLUDING code lines and code-fence marker lines. Sections, questions,
//! items and steps skip fenced code blocks (``` or ~~~, CommonMark close
//! rule: same marker, at least as long, no info string; an unclosed fence
//! runs to the end) and the fence lines themselves, so a `# comment` or a
//! `- x` inside a code block is not a heading or an item.
//!
//! Wording rules (see [`extract`]): the noun must end its clause (no
//! "of/per/each/in/with ..." after it), negated wording is not extracted,
//! and emphasis or quotes around the wording are not recognized.

/// What a `MinItems` requirement counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Items,
    Steps,
    Sections,
    Questions,
}

impl ItemKind {
    fn noun(self) -> &'static str {
        match self {
            ItemKind::Items => "list items",
            ItemKind::Steps => "numbered steps",
            ItemKind::Sections => "headings",
            ItemKind::Questions => "questions",
        }
    }
}

/// One measurable requirement on a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    MinLines(usize),
    MaxLines(usize),
    MinItems(ItemKind, usize),
    RequiredPhrases(Vec<String>),
}

impl Requirement {
    /// Short stable label, as recorded in the receipt.
    pub fn label(&self) -> String {
        match self {
            Requirement::MinLines(n) => format!("at least {n} non-empty lines"),
            Requirement::MaxLines(n) => format!("at most {n} non-empty lines"),
            Requirement::MinItems(k, n) => format!("at least {n} {}", k.noun()),
            Requirement::RequiredPhrases(p) => {
                let q: Vec<String> = p.iter().map(|x| format!("\"{x}\"")).collect();
                format!("the phrase {}", q.join(", "))
            }
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
            Requirement::MinItems(k, n) => {
                let c = count_items(*k, content);
                (c < *n).then(|| format!("{label}, found {c}"))
            }
            Requirement::RequiredPhrases(p) => {
                let low = content.to_lowercase();
                let missing: Vec<String> = p
                    .iter()
                    .filter(|x| !low.contains(&x.to_lowercase()))
                    .map(|x| format!("\"{x}\""))
                    .collect();
                (!missing.is_empty()).then(|| format!("{label}, missing {}", missing.join(", ")))
            }
        }
    }
}

fn non_empty_lines(s: &str) -> usize {
    s.lines().filter(|l| !l.trim().is_empty()).count()
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

fn clean(w: &str) -> &str {
    w.trim_matches(|c: char| matches!(c, ',' | '.' | ';' | ':' | '!' | '?' | '(' | ')'))
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
fn negated_before(raw: &[&str], at: usize) -> bool {
    for i in (at.saturating_sub(4)..at).rev() {
        if ends_clause(raw[i]) {
            return false;
        }
        if NEGATORS.contains(&clean(raw[i])) {
            return true;
        }
    }
    false
}

/// True when the clause ends right after word `at`: closing punctuation on
/// it, end of the goal, or a clause-joining word next. Anything else
/// ("of", "per", "each", "in", "with", ...) qualifies the count.
fn clause_ends_after(raw: &[&str], at: usize) -> bool {
    ends_clause(raw[at])
        || raw
            .get(at + 1)
            .is_none_or(|n| CLAUSE_JOINERS.contains(&clean(n)))
}

/// The requirements stated in `goal`, in the exact supported wordings only.
///
/// Rules, all conservative (a requirement is extracted only when the
/// wording is certainly the supported claim; otherwise it is simply not
/// extracted and never appears in `requirements_recognized`):
/// - the count noun must END its clause: closing punctuation, end of goal,
///   or a joiner (`and or but so then because that which covering`). So
///   "3 lines of context", "5 items per category", "2 sections in the
///   file" are not extracted.
/// - a negator (`not no never without avoid` and the contractions) in the
///   same clause, up to 4 words before, cancels it: "do not include the
///   phrase "X"", "not at least 20 lines". No forbidden-phrase check exists,
///   so negated wording is reported unrecognized, not inverted.
/// - the quoted phrase must also end its clause ("... "X" in the title" is
///   not extracted).
/// - emphasis or quotes around the wording (`**at least 20 lines**`,
///   `"at least 20 lines"`, `at least **20** lines`) break the exact word
///   sequence and are not recognized (known limitation).
pub fn extract(goal: &str) -> Vec<Requirement> {
    let low = goal.to_ascii_lowercase();
    let raw: Vec<&str> = low.split_whitespace().collect();
    let words: Vec<&str> = raw.iter().copied().map(clean).collect();
    let mut out: Vec<Requirement> = Vec::new();
    let mut push = |r: Requirement| {
        if !out.contains(&r) {
            out.push(r);
        }
    };
    for (i, w) in words.windows(4).enumerate() {
        if w[0] != "at" || !(w[1] == "least" || w[1] == "most") {
            continue;
        }
        if !w[2].chars().all(|c| c.is_ascii_digit()) || w[2].is_empty() {
            continue;
        }
        let Ok(n) = w[2].parse::<usize>() else {
            continue;
        };
        if n == 0 || negated_before(&raw, i) || !clause_ends_after(&raw, i + 3) {
            continue;
        }
        let least = w[1] == "least";
        match (least, w[3]) {
            (true, "lines") => push(Requirement::MinLines(n)),
            (false, "lines") => push(Requirement::MaxLines(n)),
            (true, "items" | "item") => push(Requirement::MinItems(ItemKind::Items, n)),
            (true, "steps" | "step") => push(Requirement::MinItems(ItemKind::Steps, n)),
            (true, "sections" | "section") => push(Requirement::MinItems(ItemKind::Sections, n)),
            (true, "questions" | "question") => push(Requirement::MinItems(ItemKind::Questions, n)),
            _ => {}
        }
    }
    // include|contain the phrase "X": the text is taken from the original
    // goal (case kept), between straight double quotes.
    let mut phrases: Vec<String> = Vec::new();
    for key in ["include the phrase \"", "contain the phrase \""] {
        let mut from = 0;
        while let Some(i) = low[from..].find(key) {
            let key_at = from + i;
            let start = key_at + key.len();
            let Some(len) = low[start..].find('"') else {
                break;
            };
            let end = start + len;
            // Words before the key (negation) and the first word after the
            // closing quote (qualifier), read from the same lowercase text.
            let before: Vec<&str> = low[..key_at].split_whitespace().collect();
            let negated = negated_before(&before, before.len());
            let after = low[end + 1..].split_whitespace().next();
            let closes_clause = low[end + 1..].starts_with([',', '.', ';', ':', '!', '?', ')'])
                || after.is_none_or(|n| CLAUSE_JOINERS.contains(&clean(n)));
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
            }
            from = end + 1;
        }
    }
    if !phrases.is_empty() {
        push(Requirement::RequiredPhrases(phrases));
    }
    out
}
