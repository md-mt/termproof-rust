#!/bin/sh
# Runs one real recipe inside the image and checks what came out of it.
#
# This is the image's own acceptance test, baked in rather than written in the
# workflow, so it runs identically in CI and by hand:
#
#     docker run --rm --entrypoint /opt/termproof/smoke/run-smoke.sh <image>
#
# `termproof --help` proves nothing an empty image could not also prove. What
# has to hold is that a recipe runs to the end: a pseudo-terminal is allocated,
# the external binaries the image ships are on disk and work, evidence is
# written, and every assertion passes. So the checks below are layered.
#
#   1. The recipe itself asserts, through TermProof's own assertion engine,
#      that rsvg-convert and ffmpeg ran and left files behind.
#   2. This script re-reads the evidence TermProof wrote — result.json, the
#      cast, the Markdown report, the JUnit XML — and refuses a run whose
#      verdict is anything but a clean pass.
#   3. It then inspects the two artifacts byte by byte, because "the file
#      exists" is a weaker claim than "the file is a 440x152 PNG". A
#      rasteriser that silently emits a zero-byte file on a rejected document
#      is the exact failure `evidence::screenshot` documents, and step 1 alone
#      would not catch it.
#   4. It asserts the image carries no Rust toolchain, which is a property of
#      the *build* that only the finished image can be asked about.
#
# POSIX sh, and nothing beyond coreutils, grep and od — anything else would be
# a package installed to test the image rather than to run it.
set -eu

SMOKE_DIR=${SMOKE_DIR:-/opt/termproof/smoke}
WORK=${WORK:-/tmp/termproof-smoke}
OUT=${OUT:-/tmp/termproof-smoke-runs}
JUNIT=${JUNIT:-/tmp/termproof-smoke-junit.xml}

# The dimensions of docker/smoke/panel.svg. rsvg-convert honours the SVG
# viewport, so a PNG of any other size means it rasterised something else.
WANT_W=440
WANT_H=152
# A 440x152 panel of flat background compresses to a few hundred bytes; the
# same panel with 85 glyphs on it does not. This is the cheapest available
# signal that ink actually landed rather than that a canvas was allocated.
MIN_PNG_BYTES=1200

fail() {
    echo "smoke: FAIL: $*" >&2
    exit 1
}

pass() {
    echo "smoke: ok: $*"
}

# --- 0. prepare a writable working directory -------------------------------
# The recipe names /tmp/termproof-smoke as its cwd and reads panel.svg from
# it. Copying rather than running in place keeps the image's own copy pristine
# when the container is reused.
rm -rf "$WORK" "$OUT" "$JUNIT"
mkdir -p "$WORK"
cp "$SMOKE_DIR/panel.svg" "$WORK/panel.svg"

# --- 1. run the recipe -----------------------------------------------------
echo "smoke: running $SMOKE_DIR/container-image.recipe.json"
if ! termproof run "$SMOKE_DIR/container-image.recipe.json" --out "$OUT" --xml-path "$JUNIT"; then
    echo "--- evidence from the failed run ---" >&2
    find "$OUT" -type f \( -name 'result.json' -o -name 'screen.txt' -o -name '*.md' \) \
        -exec sh -c 'echo "== $1"; cat "$1"' _ {} \; >&2 2>/dev/null || true
    fail "termproof run exited non-zero"
fi
pass "termproof run exited 0"

# --- 2. the evidence TermProof wrote ---------------------------------------
RUN_DIR=$(find "$OUT" -mindepth 1 -maxdepth 1 -type d | head -n 1)
[ -n "$RUN_DIR" ] || fail "no run directory under $OUT"

for artifact in result.json report.md raw_output.txt screen.txt session.cast; do
    [ -s "$RUN_DIR/$artifact" ] || fail "$artifact is missing or empty in $RUN_DIR"
done
pass "run directory carries result.json, report.md, raw_output.txt, screen.txt and session.cast"

# A recipe that never started still writes a result. The verdict is the field
# that distinguishes the two, and every assertion has to have reached it.
grep -q '"passed": true' "$RUN_DIR/result.json" || fail "result.json does not report a pass"
if grep -q '"passed": false' "$RUN_DIR/result.json"; then
    cat "$RUN_DIR/result.json" >&2
    fail "at least one step or assertion failed"
fi
pass "result.json: every step and assertion passed"

# The six assertions the recipe declares, plus the synthetic exit_code one.
ASSERTED=$(grep -c '"passed"' "$RUN_DIR/result.json")
[ "$ASSERTED" -ge 10 ] || fail "expected at least 10 verdicts in result.json, found $ASSERTED"
pass "result.json carries $ASSERTED step and assertion verdicts"

for phrase in SMOKE-OK rasterised encoded; do
    grep -q "$phrase" "$RUN_DIR/screen.txt" || fail "screen.txt does not mention '$phrase'"
done
pass "screen.txt shows the script reached SMOKE-OK"

# The cast is what a later `agg`/`cast_video` pass would consume, so its header
# has to be a real asciinema v2 one and not an empty file that happens to exist.
head -n 1 "$RUN_DIR/session.cast" | grep -q '"version": *2' \
    || fail "session.cast has no asciinema v2 header"
pass "session.cast has an asciinema v2 header"

[ -s "$OUT/latest-report.md" ] || fail "latest-report.md was not written"
grep -q 'container-image-smoke' "$OUT/latest-report.md" \
    || fail "latest-report.md does not name the recipe"
pass "latest-report.md names the recipe"

# Asserted by the absence of the elements rather than by `failures="0"`, so a
# change in how quick-junit renders its counters cannot turn a passing image
# into a red build.
[ -s "$JUNIT" ] || fail "JUnit XML was not written to $JUNIT"
grep -q 'container-image-smoke' "$JUNIT" || fail "JUnit XML has no testcase for the recipe"
if grep -q '<failure\|<error' "$JUNIT"; then
    cat "$JUNIT" >&2
    fail "JUnit XML carries a failure or error element"
fi
pass "JUnit XML has the testcase and no failure or error element"

# --- 3. the artifacts the external binaries produced ------------------------
PNG="$WORK/panel.png"
MP4="$WORK/clip.mp4"

[ -s "$PNG" ] || fail "rsvg-convert produced no PNG (is librsvg2-bin installed?)"
# The first four bytes are the PNG signature \x89PNG; bytes 16..23 are the
# IHDR width and height, big-endian. Read as unsigned decimal rather than as
# characters, because `od -c` renders a high byte differently on GNU and BSD
# and this script is run by hand on both.
# `NF>=4 … exit` because BSD od emits a trailing line that GNU od does not,
# and awk would otherwise print a second, zero answer for it. `printf "%.0f"`
# rather than `print` because mawk — the awk in a Debian base image — falls
# back to OFMT for a value past INT_MAX, so the PNG signature 2303741511 comes
# out as "2.30374e+09" and every comparison against it fails.
be32() {
    od -An -tu1 -j "$2" -N4 "$1" |
        awk 'NF>=4 {printf "%.0f\n", $1*16777216 + $2*65536 + $3*256 + $4; exit}'
}
[ "$(be32 "$PNG" 0)" = "2303741511" ] || fail "panel.png does not start with the PNG signature"
GOT_W=$(be32 "$PNG" 16)
GOT_H=$(be32 "$PNG" 20)
[ "$GOT_W" = "$WANT_W" ] && [ "$GOT_H" = "$WANT_H" ] \
    || fail "panel.png is ${GOT_W}x${GOT_H}, expected ${WANT_W}x${WANT_H} — rsvg-convert did not honour the SVG viewport"
PNG_BYTES=$(wc -c < "$PNG" | tr -d ' ')
[ "$PNG_BYTES" -ge "$MIN_PNG_BYTES" ] \
    || fail "panel.png is only $PNG_BYTES bytes — the panel rasterised blank"
pass "rsvg-convert produced a ${GOT_W}x${GOT_H} PNG of $PNG_BYTES bytes"

[ -s "$MP4" ] || fail "ffmpeg produced no MP4 (is ffmpeg installed?)"
# Every ISO base media file names its brand in an `ftyp` box at offset 4.
head -c 12 "$MP4" | LC_ALL=C grep -aq 'ftyp' || fail "clip.mp4 has no ftyp box"
MP4_BYTES=$(wc -c < "$MP4" | tr -d ' ')
[ "$MP4_BYTES" -ge 1024 ] || fail "clip.mp4 is only $MP4_BYTES bytes"
pass "ffmpeg produced an MP4 of $MP4_BYTES bytes"

# --- 4. the image is a runtime image, not a build image ---------------------
# A multi-stage build that accidentally ships its builder stage still passes
# every check above. This is the one that would catch it.
for tool in cargo rustc rustup; do
    if command -v "$tool" >/dev/null 2>&1; then
        fail "$tool is on PATH — the runtime stage is carrying the Rust toolchain"
    fi
done
[ ! -d /usr/local/rustup ] || fail "/usr/local/rustup exists in the runtime image"
[ ! -d /usr/local/cargo ] || fail "/usr/local/cargo exists in the runtime image"
pass "no Rust toolchain in the runtime image"

echo "smoke: all checks passed"
