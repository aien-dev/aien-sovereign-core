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

fn count_items(k: ItemKind, s: &str) -> usize {
    s.lines()
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

/// The requirements stated in `goal`, in the exact supported wordings only.
pub fn extract(goal: &str) -> Vec<Requirement> {
    let low = goal.to_ascii_lowercase();
    let words: Vec<&str> = low.split_whitespace().map(clean).collect();
    let mut out: Vec<Requirement> = Vec::new();
    let mut push = |r: Requirement| {
        if !out.contains(&r) {
            out.push(r);
        }
    };
    for w in words.windows(4) {
        if w[0] != "at" || !(w[1] == "least" || w[1] == "most") {
            continue;
        }
        if !w[2].chars().all(|c| c.is_ascii_digit()) || w[2].is_empty() {
            continue;
        }
        let Ok(n) = w[2].parse::<usize>() else {
            continue;
        };
        if n == 0 {
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
            let start = from + i + key.len();
            let Some(len) = low[start..].find('"') else {
                break;
            };
            if len > 0 && goal.is_char_boundary(start) && goal.is_char_boundary(start + len) {
                let p = goal[start..start + len].to_string();
                if !phrases.contains(&p) {
                    phrases.push(p);
                }
            }
            from = start + len + 1;
        }
    }
    if !phrases.is_empty() {
        push(Requirement::RequiredPhrases(phrases));
    }
    out
}
