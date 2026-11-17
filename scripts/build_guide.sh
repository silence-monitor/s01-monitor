#!/bin/bash
# Makes the user guide PDF from docs/user-guide.html and docs/images/.
#
#   scripts/build_guide.sh OUT.pdf VERSION "DATE"
#
# It uses Google Chrome in headless mode (Chrome is already on the Mac; nothing
# is installed). Set CHROME to use another Chromium-based browser.
# @VERSION@ and @DATE@ in the page are filled in. The page and its pictures are
# copied to a temporary folder first, so nothing in docs/ changes.
set -euo pipefail
cd "$(dirname "$0")/.."

[ $# -eq 3 ] || { echo "usage: scripts/build_guide.sh OUT.pdf VERSION \"DATE\"" >&2; exit 2; }
case "$1" in /*) OUT="$1" ;; *) OUT="$PWD/$1" ;; esac
VERSION="$2"
DATE="$3"

fail() { echo "STOPPED: $*" >&2; exit 1; }

CHROME="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
[ -x "$CHROME" ] || fail "Google Chrome was not found at '$CHROME' (set CHROME to a Chromium-based browser)."
[ -f docs/user-guide.html ] || fail "docs/user-guide.html is missing."
[ -d docs/images ] || fail "docs/images is missing."

# The page itself must follow the rules before it is printed.
LOCAL_USER=$(basename "$HOME")
if grep -qi -- "$LOCAL_USER" docs/user-guide.html; then fail "the guide mentions the user name '$LOCAL_USER'."; fi
if grep -q '/Use''rs/' docs/user-guide.html; then fail "the guide contains a home folder path."; fi
for image in docs/images/*.png; do
  [ -s "$image" ] || fail "$image is empty."
done
for name in $(grep -o 'images/[A-Za-z0-9_-]*\.png' docs/user-guide.html | sort -u); do
  [ -f "docs/$name" ] || fail "the guide uses docs/$name, which does not exist."
done

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cp -R docs/images "$work/images"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$DATE/g" docs/user-guide.html > "$work/user-guide.html"
grep -q '@VERSION@\|@DATE@' "$work/user-guide.html" && fail "a placeholder was left in the page."

mkdir -p "$(dirname "$OUT")"
rm -f "$OUT"
"$CHROME" --headless=new --disable-gpu --no-pdf-header-footer --no-sandbox \
  --user-data-dir="$work/profile" --print-to-pdf="$OUT" "file://$work/user-guide.html" >/dev/null 2>&1 || true

# Check what came out.
[ -s "$OUT" ] || fail "Chrome did not make a PDF."
[ "$(head -c 5 "$OUT")" = "%PDF-" ] || fail "the output is not a PDF."
SIZE=$(wc -c < "$OUT" | tr -d ' ')
[ "$SIZE" -gt 300000 ] || fail "the PDF is only $SIZE bytes; the pictures are probably missing."
[ "$(strings -a "$OUT" | grep -ci -- "$LOCAL_USER" || true)" -eq 0 ] || fail "the PDF mentions the user name '$LOCAL_USER'."
[ "$(strings -a "$OUT" | grep -c '/Use''rs/' || true)" -eq 0 ] || fail "the PDF contains a home folder path."
echo "Guide: $OUT ($((SIZE / 1024)) KB)"
