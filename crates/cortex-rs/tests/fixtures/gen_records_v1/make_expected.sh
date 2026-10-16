#!/bin/sh
# make_expected.sh <genret> <recheck> <out dir>: regenerates the reference outputs the Cortex GEN-5 tests
# (crates/cortex-rs/tests/gen_procedures.rs) compare against, using the REAL omega tools built from the
# pinned commits in PINS (never from this tree). Writes only into <out dir>, which must not exist; copy
# <out dir>/expected and <out dir>/recheck over the vendored ones after reviewing the diff.
#   expected/<case>/  genret's six output files for each row of cases.tsv (genret-cli-v3)
#   expected/refusals.tsv  genret's fatal reason for each refused input (corpus mutations, bad options)
#   OUTPUTS.sha256         sha256 of every file under expected/ and recheck/ (its own digest goes in PINS)
#   RECHECK_SHA256  sha256 of <recheck>, passed as --recheck-sha256 (and by the tests as recheck_sha256)
#   recheck/<name>.rec and recheck/verdicts.tsv: each mutant of mutants.tsv and recheck's verdict on it
#     ("OK <id>" or "REFUSED line <n>: <reason>", the file name removed)
# POSIX sh plus sed, awk, sha256sum. No network, no hidden task files.
set -eu
G="$1" RC="$2" OUT="$3"
case "$G" in /*) ;; *) G="$(pwd)/$G" ;; esac
case "$RC" in /*) ;; *) RC="$(pwd)/$RC" ;; esac
HERE="$(cd "$(dirname "$0")" && pwd)"
LC_ALL=C; export LC_ALL
[ ! -e "$OUT" ] || { echo "make_expected: $OUT exists" >&2; exit 2; }
REC="$HERE/corpus/records"
MAN="$REC/MANIFEST"
PIN="$(cut -c1-64 "$REC/SET.sha256")"
[ "$(sha256sum "$MAN" | cut -c1-64)" = "$PIN" ] || { echo "make_expected: MANIFEST does not match SET.sha256" >&2; exit 2; }
mkdir -p "$OUT/expected" "$OUT/recheck" "$OUT/work"
RSHA="$(sha256sum "$RC" | cut -c1-64)"
printf "%s\n" "$RSHA" > "$OUT/RECHECK_SHA256"

# ---- genret reference outputs, one directory per case ----
tail -n +2 "$HERE/cases.tsv" | while IFS="$(printf '\t')" read -r name kind task k cap mode target excl; do
  set -- --mode "$mode"
  [ "$target" = - ] || set -- "$@" --target-bytes "$target"
  if [ "$excl" != - ]; then
    for f in $(printf '%s' "$excl" | tr ',' ' '); do set -- "$@" --exclude-family "$f"; done
  fi
  "$G" query --corpus "$MAN" --manifest-sha256 "$PIN" --recheck "$RC" --recheck-sha256 "$RSHA" --kind "$kind" --k "$k" --cap-bytes "$cap" \
    --requirement "$HERE/tasks/$task/requirement" --prompt "$HERE/tasks/$task/prompt" --out "$OUT/expected/$name" "$@"
done

# ---- recheck verdicts on schema mutants ----
# mutants.tsv: <name> TAB <base record path under corpus/records> TAB <sed program> ('-' = unchanged).
# Byte-level mutants that sed cannot express portably are written below with printf.
tail -n +2 "$HERE/mutants.tsv" | while IFS="$(printf '\t')" read -r name base prog; do
  if [ "$prog" = - ]; then cp "$REC/$base" "$OUT/recheck/$name.rec"
  else sed -e "$prog" "$REC/$base" > "$OUT/recheck/$name.rec"; fi
done
A="$REC/abstraction/abs-0054ea1b5f8a9efb.rec"
sed -e '3s/$/\r/' "$A" > "$OUT/recheck/cr_byte.rec"   # GNU sed: \r is CR
awk 'NR == 10 { sub(/^desc Helper:/, "desc Helper:\t") } { print }' "$A" > "$OUT/recheck/tab_byte.rec"
awk 'NR == 10 { sub(/^desc Helper:/, "desc Helper:\303\251") } { print }' "$A" > "$OUT/recheck/non_ascii.rec"
awk 'NR == 2 { print; print ""; next } { print }' "$A" > "$OUT/recheck/empty_line.rec"
head -c -1 "$A" > "$OUT/recheck/no_final_lf.rec"
awk 'NR == 10 { $0 = "desc " sprintf("%161s", ""); gsub(/ /, "x"); sub(/^descx/, "desc ") } { print }' "$A" > "$OUT/recheck/desc_161.rec"
awk '/^ancestry / { a[++n] = $0; if (n == 2) { print a[2]; print a[1] }; next } { print }' "$A" > "$OUT/recheck/ancestry_unsorted.rec"
awk '/^ret k$/ { print "id raw-evil"; print "validity verified"; print "split corpus" } { print }' "$A" > "$OUT/recheck/body_text_injection.rec"
for f in "$OUT"/recheck/*.rec; do
  n="$(basename "$f" .rec)"
  set +e; v="$("$RC" --receipts "$REC/receipts" "$f")"; set -e
  printf '%s\t%s\n' "$n" "$(printf '%s' "$v" | sed "s|^REFUSED $f: |REFUSED |")"
done > "$OUT/recheck/verdicts.tsv"

# ---- genret refusals (exit 2, no output): reason word per case, mirrored by tests/gen_procedures.rs ----
T="$HERE/tasks/devq_clamp"
CR="$(sed -n 's/^receipt //p' "$REC/raw/raw-clamp_sub.rec")"
DR="$(sed -n 's/^receipt //p' "$REC/raw/raw-dot3.rec")"
reseal() { # rewrite MANIFEST digests in place (same order) and print the new pin
  (cd "$1" && cut -c67- MANIFEST | while IFS= read -r p; do sha256sum "$p"; done > MANIFEST.new && mv MANIFEST.new MANIFEST &&
   sha256sum MANIFEST | cut -c1-64 | tee SET.sha256.tmp && { cat SET.sha256.tmp > SET.sha256; rm SET.sha256.tmp; })
}
refuse() { # refuse <name> <records dir> <pin> [extra genret options...]
  n="$1" d="$2" p="$3"; shift 3
  set +e
  e="$("$G" query --corpus "$d/MANIFEST" --manifest-sha256 "$p" --recheck "$RC" --recheck-sha256 "$RSHA" \
      --kind raw --k 3 --cap-bytes 4096 --requirement "${RQ:-$T/requirement}" --prompt "${PR:-$T/prompt}" \
      --out "$OUT/work/$n.out" "$@" 2>&1 >/dev/null)"; rc=$?
  set -e
  [ "$rc" = 2 ] && [ ! -e "$OUT/work/$n.out" ] || { echo "make_expected: $n: exit $rc" >&2; exit 2; }
  printf '%s\t%s\n' "$n" "$(printf '%s' "$e" | awk 'NR == 1 { print $3 }')"
}
fresh() { rm -rf "$OUT/work/$1"; cp -R "$REC" "$OUT/work/$1"; printf '%s' "$OUT/work/$1"; }
{
  d="$(fresh stale_digest)"; printf 'end\n' >> "$d/raw/raw-dot3.rec"; refuse stale_digest "$d" "$PIN"
  refuse wrong_pin "$REC" 0000000000000000000000000000000000000000000000000000000000000000
  d="$(fresh manifest_bad_path)"; sed -i '1s|  .*|  ../x.rec|' "$d/MANIFEST"
  refuse manifest_bad_path "$d" "$(sha256sum "$d/MANIFEST" | cut -c1-64)"
  d="$(fresh record_missing)"; rm "$d/raw/raw-dot3.rec"; refuse record_missing "$d" "$PIN"
  d="$(fresh receipt_forged)"; printf 'note forged\n' >> "$d/receipts/$CR"; refuse receipt_forged "$d" "$PIN"
  d="$(fresh receipt_missing)"; rm "$d/receipts/$CR"; refuse receipt_missing "$d" "$PIN"
  d="$(fresh receipt_borrowed)"; sed -i "s/^receipt $CR\$/receipt $DR/" "$d/raw/raw-clamp_sub.rec"
  refuse receipt_borrowed "$d" "$(reseal "$d")"
  d="$(fresh probe_abstraction)"; sed -i 's/^split corpus$/split probe/' "$d/abstraction/abs-0054ea1b5f8a9efb.rec"
  refuse probe_abstraction "$d" "$(reseal "$d")"
  d="$(fresh duplicate_id)"; cp "$d/raw/raw-dot3.rec" "$d/raw/raw-dot3b.rec"
  printf '%s  raw/raw-dot3b.rec\n' "$(sha256sum "$d/raw/raw-dot3b.rec" | cut -c1-64)" >> "$d/MANIFEST"
  refuse duplicate_id "$d" "$(reseal "$d")"
  refuse usage_k0 "$REC" "$PIN" --k 0
  refuse usage_target_over_cap "$REC" "$PIN" --mode length-matched --target-bytes 5000
  mkdir -p "$OUT/work/hidden"; cp "$T/requirement" "$OUT/work/hidden/requirement"
  RQ="$OUT/work/hidden/requirement" refuse hidden_path "$REC" "$PIN"
  printf 'tab\there\n' > "$OUT/work/prompt_tab"; PR="$OUT/work/prompt_tab" refuse prompt_tab "$REC" "$PIN"
} > "$OUT/expected/refusals.tsv"
rm -rf "$OUT/work"
# ---- pin every produced output: OUTPUTS.sha256 (sha256sum format, C-sorted paths) ----
# tests/gen_procedures.rs checks every expected/ and recheck/ file against it and the file set exactly, and
# PINS records the digest of OUTPUTS.sha256 itself, so the outputs cannot be silently regenerated from Rust.
(cd "$OUT" && find expected recheck -type f | LC_ALL=C sort | while IFS= read -r p; do sha256sum "$p"; done > OUTPUTS.sha256)
