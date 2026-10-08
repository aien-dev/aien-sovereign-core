# Brief given to the independent v5 task author (verbatim)

Given on 2026-10-08 by session fb5693 to a Sonnet 5.5 worker. The worker's one tool call wrote `tasks.json` and the seeds; its
goals, destinations and seeds are committed unchanged in `tasks-oq3-v5.json` and `seed-v5/` (the spec author changed only the two
topic-form lists recorded in ACCEPTANCE-v5 Section 3.1, and the `phrases` of D3, E1, E2 and R1, which the scorer's inherited Q2 row
reads; every phrase is a substring of its goal). **The requirement kinds per task were given by the spec author** and mirror v4's
kinds on purpose, including the four kinds v4 found refused, so that v5 checks the product fixes on fresh wording. The wording of
every goal is the author's.

---

You are writing FRESH test tasks for a qualification campaign of a local document-writing AI assistant. A user gives the assistant a one-paragraph goal in plain English ("Create the file docs/X.md ..."); the assistant writes or edits one file in a workspace. Your goals will later be checked mechanically against the saved file.

INDEPENDENCE RULE (most important): do NOT open, read, grep or list anything under /home/drakestapleton/.claude/jobs/a7c5d201/tmp/sc-dec, ~/workspace, or any git repository on this machine, and do not search the web for the product. You must write the goals the way a real user would phrase them, without knowing how the product parses goals. Write only into /home/drakestapleton/.claude/jobs/a7c5d201/tmp/v5-tasks/ (create it with mkdir -p). Do not run cargo or any build. No GitHub actions.

Write these 10 tasks (ids fixed). Every goal: one paragraph, plain natural English as a normal person would type it, varied wording between tasks (do not reuse one sentence template), no backticks, no pipe characters "|", no line breaks. Every measurable requirement must be stated explicitly in the goal (a number, exact section titles in double quotes, exact words), so a checker can test it on the saved file without judgement. Use some numbers as digits and some as words (e.g. "fifteen").

- D1 (new document): a how-to guide. Requirements: a minimum number of non-empty lines (pick 20 to 26), three sections with exact titles in double quotes, and two exact single words that must appear.
- D2 (new document): technical how-to with code. Requirements: a minimum line count (18 to 24), a stated number (2 or 3) of fenced code examples, each introduced by a section with an exact quoted title, and a final section with an exact quoted title, placed after the last code example, that holds at least a stated number (10 to 15) of words of plain text.
- D3 (new document): an explainer that must cover a stated number of named topics (3 or 4 topics, each named by one word). Also a minimum line count (16 to 20). State explicitly that the topic counts as covered when its word, or another form of that word, appears; give example forms. Then list, for the JSON (not in the goal), the full set of accepted whole-word forms for each topic (base, plural, past, -ing, and both British/American spellings where relevant).
- D4 (new document created from an existing file): the goal asks to create a NEW file that summarises an EXISTING seed file at a different path (e.g. notes/xyz.txt). Write the seed file too (12 to 20 lines, plain text, mentions three people by first name). Requirements: minimum lines (12 to 16), two exact quoted section titles, and the three first names must appear.
- D5 (new document): a checklist. Requirements: a minimum AND a maximum line count, every item written as a line starting with "- [ ]" (state that prefix in double quotes), at least N such items (N between 10 and 14; refer to the items naturally, however you like), three exact quoted section titles, and two exact words that must appear.
- D6 (new document): a longer piece of prose that must have at least a stated number of paragraphs (6 to 8) and one exact quoted section title.
- E1 (small edit): add one stated bullet line (in double quotes, starting with "- ") to a named section of an existing markdown file. Write the seed file: at least two level-two headings ("## ..."), 2 to 4 bullet lines under each.
- E2 (small edit, nested path like a/b/FILE.md): add one stated numbered item (e.g. "4. ...") under a named heading of an existing markdown file with a numbered list. Write the seed (two or more "## " headings).
- N1 (should be refused): ask to create a file whose path is outside the workspace (starts with ../), with a single stated line of content.
- N2 (long request): ask for a long new document (e.g. a long history or report) with at least a stated number of paragraphs (10 to 14). It will be run with a tiny output cap on purpose.
- R1 (tiny new plain-text file, .txt): a one-sentence note (say "one-sentence" or "a single sentence" naturally).

Topics FORBIDDEN (already used in earlier campaigns; do not use these subjects or file names): build pipeline, release checklist, onboarding, roadmap, operations/log rotation weekly, handover, project overview, hello/greeting, composting, backups, seed saving, standup/week summary, camping, garage/car, chores, thank-you note, project history, user guide/installation, troubleshooting, architecture of a CLI tool, contributing, FAQ, herb garden, bicycle chain, changelog, TODO/buy stamps, plants needing water or sun. Pick fresh everyday subjects (for example cooking, home repair, pets, music practice, travel, money, sports, crafts), each task a different subject. File names must not be any of: GUIDE, TROUBLESHOOTING, ARCHITECTURE, CONTRIBUTING, FAQ, NOTES, PIPELINE, RELEASE-CHECKLIST, ONBOARDING, ROADMAP, OPERATIONS, OVERVIEW, HELLO, HISTORY, COMPOST, BACKUPS, SEED-SAVING, WEEK-SUMMARY, CAMPING-LIST, GARAGE, CHORES, THANKS, CHANGELOG, TODO.

Deliver:
1. /home/drakestapleton/.claude/jobs/a7c5d201/tmp/v5-tasks/tasks.json: an array of objects {id, kind ("document"|"edit"|"negative"|"replay"), goal (exact text), destination (relative path, or the ../ path for N1), seed ({path, file} or null), requirements: [ {kind: one of min_lines|max_lines|headings|word|topic|code_blocks|tail|prefixed_lines|paragraphs|sentences_exact, as_written: the exact substring of the goal that states it, n: number when there is one, titles/words/forms/prefix/heading as relevant} ], phrases: [every literal string the goal states that must appear in the saved file] }. For edits, requirements may be empty; put the new line and the heading in phrases. Every as_written MUST be an exact substring of the goal; every phrase MUST be an exact (case-insensitive) substring of the goal.
2. Seed files under /home/drakestapleton/.claude/jobs/a7c5d201/tmp/v5-tasks/seed/<ID>/<path>.
3. Check yourself with jq/grep that every as_written and phrase is literally in its goal and that no goal contains a backtick or "|". Report: the list of goals (verbatim), and the result of your self-check. Keep the report short.
