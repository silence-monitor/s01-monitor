#!/bin/bash
# Builds the release packages of the monitor, following the project's rules,
# and refuses to produce one that breaks them.
#
#   scripts/release.sh TARGET [--dist DIR] [--skip-tests] [--allow-dirty] [--force]
#
#   TARGET          windows : dist-windows/s01-monitor-<version>-windows.zip
#                   mac     : dist-mac/s01-monitor-<version>-mac-arm64.zip (a .app)
#                             (both zips carry the user guide, a PDF)
#                   esp     : two packages, one per hardware, each a folder and a zip:
#                             dist-esp/s01-monitor-<version>-esp-canbase   (ATOM Lite + Atomic CAN Base)
#                             dist-esp/s01-monitor-<version>-esp-grovecan-tfbase  (ATOM Lite + Unit CAN on the Grove port + TF Card Base)
#                   all     : all three
#   --dist DIR      put the output in DIR instead of dist-<target>
#   --skip-tests    do not run the tests first
#   --allow-dirty   build even though there are uncommitted changes
#   --force         replace a package of the same version
#
# It builds and packages only. It never flashes the ESP32.
#
# The rules it keeps:
#  - the version comes from the top-level Cargo.toml and the newest CHANGELOG
#    entry must be for that version
#  - the programs are built with paths remapped, and checked, so they hold no
#    user name or folder
#  - a package holds only what it should (see each target) and a CHANGELOG.md;
#    the Windows and Mac packages also hold User-Guide.pdf, made from docs/ by
#    scripts/build_guide.sh. No capture log, no personal data
#  - a README has no "What to tell me" section
#  - a package of the same
#    version is not replaced unless --force is given
set -euo pipefail
cd "$(dirname "$0")/.."

TARGET=""
DIST_OVERRIDE=""
RUN_TESTS=1
ALLOW_DIRTY=0
FORCE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --dist) [ $# -ge 2 ] || { echo "--dist needs a folder" >&2; exit 2; }; DIST_OVERRIDE="$2"; shift ;;
    --skip-tests) RUN_TESTS=0 ;;
    --allow-dirty) ALLOW_DIRTY=1 ;;
    --force) FORCE=1 ;;
    -h|--help) sed -n '2,26p' "$0"; exit 0 ;;
    -*) echo "Unknown option: $1 (try --help)" >&2; exit 2 ;;
    *) [ -z "$TARGET" ] || { echo "Only one target at a time: $TARGET or $1" >&2; exit 2; }; TARGET="$1" ;;
  esac
  shift
done
case "$TARGET" in
  windows|mac|esp) TARGETS=("$TARGET") ;;
  all) TARGETS=(windows mac esp) ;;
  "") echo "Say what to build: windows, mac, esp or all (try --help)" >&2; exit 2 ;;
  *) echo "Unknown target: $TARGET (windows, mac, esp or all)" >&2; exit 2 ;;
esac

fail() { echo "STOPPED: $*" >&2; exit 1; }
step() { echo; echo "== $*"; }
need() { for tool in "$@"; do command -v "$tool" >/dev/null || fail "'$tool' is not installed."; done; }

LOCAL_USER=$(basename "$HOME")
# The project folder is inside the home folder. Remap it too (rustc lets the
# last matching remap win), so the program does not carry the folder's name.
ROOT_REL="${PWD#"$HOME"/}"
REMAP="--remap-path-prefix=$HOME=/build --remap-path-prefix=$PWD=/src"
DATE=$(date '+%e %b %Y' | sed 's/^ //')
C_ORDER() { LC_ALL=C sort; }

dist_for() { if [ -n "$DIST_OVERRIDE" ]; then echo "$DIST_OVERRIDE"; else echo "dist-$1"; fi; }
name_for() {
  case "$1" in
    windows) echo "s01-monitor-$VERSION-windows" ;;
    mac) echo "s01-monitor-$VERSION-mac-arm64" ;;
  esac
}
# The ESP32 has one package per hardware.
ESP_VARIANTS=(canbase grovecan-tfbase)
esp_name() { echo "s01-monitor-$VERSION-esp-$1"; }
esp_label() {
  case "$1" in
    canbase) echo "ESP32 firmware, ATOM Lite + Atomic CAN Base" ;;
    grovecan-tfbase) echo "ESP32 firmware, ATOM Lite + Unit CAN (Grove) + TF Card Base" ;;
  esac
}
# The name the image carries in its boot log, and the other one it must not carry.
esp_board_in() {
  case "$1" in
    canbase) echo "M5 ATOM Lite + Atomic CAN Base" ;;
    grovecan-tfbase) echo "M5 ATOM Lite + Unit CAN (Grove) + TF Card Base" ;;
  esac
}

# --- the rules that need no build ---------------------------------------

VERSION=$(awk '
  /^\[workspace.package\]/ { inside = 1; next }
  /^\[/ { inside = 0 }
  inside && /^version[ ]*=/ { gsub(/[" ]/, "", $0); sub(/^version=/, "", $0); print; exit }
' Cargo.toml)
[ -n "$VERSION" ] || fail "could not read the version from Cargo.toml."
echo "Version $VERSION, building: ${TARGETS[*]}"

NEWEST=$(grep -m1 '^## ' CHANGELOG.md | sed -E 's/^## +([^ ]+).*/\1/')
[ "$NEWEST" = "$VERSION" ] || fail "the newest CHANGELOG.md entry is '$NEWEST', but the version is $VERSION."

for t in "${TARGETS[@]}"; do
  if [ "$t" = esp ]; then
    names=(); for v in "${ESP_VARIANTS[@]}"; do names+=("$(esp_name "$v")"); done
  else
    names=("$(name_for "$t")")
  fi
  for n in "${names[@]}"; do
    folder="$(dist_for "$t")/$n"
    if [ "$FORCE" -eq 0 ] && { [ -e "$folder" ] || [ -e "$folder.zip" ]; }; then
      fail "$folder already exists. Use --force to replace it, or raise the version."
    fi
  done
done

if [ "$ALLOW_DIRTY" -eq 0 ]; then
  if [ -n "$(git status --porcelain)" ]; then
    git status --short >&2
    fail "there are uncommitted changes, so the package would not match any commit. Commit first, or use --allow-dirty."
  fi
  TAG_AT=$(git rev-parse -q --verify "refs/tags/v$VERSION^{commit}" || true)
  if [ -n "$TAG_AT" ] && [ "$TAG_AT" != "$(git rev-parse HEAD)" ]; then
    fail "tag v$VERSION points at another commit than HEAD."
  fi
  [ -n "$TAG_AT" ] || echo "Note: there is no tag v$VERSION yet."
else
  echo "Note: building with --allow-dirty; this package may not match a commit."
fi

# --- tools, before anything is built ------------------------------------
need_chrome() {
  [ -x "${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}" ] \
    || fail "Google Chrome is needed to make the user guide PDF (or set CHROME to a Chromium-based browser)."
}
need cargo shasum strings
for t in "${TARGETS[@]}"; do
  case "$t" in
    windows)
      need_chrome
      need zig zip unzip
      cargo zigbuild --help >/dev/null 2>&1 || fail "cargo-zigbuild is not installed (cargo install cargo-zigbuild --locked)."
      rustup target list --installed 2>/dev/null | grep -qx 'x86_64-pc-windows-gnu' \
        || fail "the Rust target is missing (rustup target add x86_64-pc-windows-gnu)." ;;
    mac)
      need_chrome
      [ "$(uname -s)" = Darwin ] || fail "the Mac app can only be built on a Mac."
      [ "$(uname -m)" = arm64 ] || fail "this builds the Apple silicon app, so it needs an Apple silicon Mac."
      need codesign iconutil sips ditto plutil python3 unzip ;;
    esp)
      IDF_EXPORT="${IDF_PATH:-$HOME/esp-idf}/export.sh"
      [ -f "$IDF_EXPORT" ] || fail "ESP-IDF was not found at $IDF_EXPORT (install v5.3.2, see firmware/README.md)."
      need wasm-bindgen python3
      rustup target list --installed 2>/dev/null | grep -qx 'wasm32-unknown-unknown' \
        || fail "the Rust target is missing (rustup target add wasm32-unknown-unknown)." ;;
  esac
done

# --- tests, once --------------------------------------------------------
if [ "$RUN_TESTS" -eq 1 ]; then
  step "Tests"
  cargo test -p s01-monitor 2>&1 | tail -n 4
  case " ${TARGETS[*]} " in
    *" esp "*) scripts/test_firmware_filter.sh 2>&1 | tail -n 1 ;;
  esac
else
  echo "Note: tests skipped."
fi

# --- shared checks ------------------------------------------------------
# A program must not carry the user name (the path remapping keeps it out).
check_no_user() { # file what
  local leaks
  leaks=$(strings -a "$1" | grep -ci -- "$LOCAL_USER" || true)
  [ "$leaks" -eq 0 ] || fail "$2 mentions the user name '$LOCAL_USER' $leaks times."
}

# The README and CHANGELOG in a folder follow the rules.
check_texts() { # folder
  grep -qi 'what to tell me' "$1/README.txt" && fail "the README has a 'What to tell me' section."
  grep -qi -- "$LOCAL_USER" "$1/README.txt" "$1/CHANGELOG.md" && fail "the README or CHANGELOG mentions the user name '$LOCAL_USER'."
  grep -q '/Use''rs/' "$1/README.txt" "$1/CHANGELOG.md" && fail "the README or CHANGELOG contains a home folder path."
  return 0
}

# A folder holds exactly these names (given in C-locale order, with |).
check_listing() { # folder expected
  local listed
  listed=$(cd "$1" && LC_ALL=C ls -1 | paste -sd'|' -)
  [ "$listed" = "$2" ] || fail "the folder holds '$listed', not '$2'."
}

write_readme() { # folder template label
  local heading="Silence S01 monitor $VERSION - $3 ($DATE)"
  {
    echo "$heading"
    printf '%*s\n' "${#heading}" '' | tr ' ' '='
    echo
    cat "$2"
  } > "$1/README.txt"
}

# The user guide, made once per run and copied into each package that has it.
GUIDE=""
GUIDE_DIR=""
cleanup() { [ -z "$GUIDE_DIR" ] || rm -rf "$GUIDE_DIR"; }
trap cleanup EXIT
ensure_guide() {
  [ -z "$GUIDE" ] || return 0
  step "User guide"
  GUIDE_DIR=$(mktemp -d)
  scripts/build_guide.sh "$GUIDE_DIR/User-Guide.pdf" "$VERSION" "$DATE" || fail "the user guide did not build."
  GUIDE="$GUIDE_DIR/User-Guide.pdf"
}

fresh_folder() { # dist folder
  mkdir -p "$1"
  if [ "$FORCE" -eq 1 ]; then rm -rf "$2" "$2.zip"; fi
  mkdir "$2"
}

# --- Windows ------------------------------------------------------------
build_windows() {
  local dist folder zip exe
  dist=$(dist_for windows); folder="$dist/$(name_for windows)"; zip="$folder.zip"
  step "Windows: building the exe"
  RUSTFLAGS="$REMAP" cargo zigbuild --release --target x86_64-pc-windows-gnu -p s01-monitor
  exe="target/x86_64-pc-windows-gnu/release/s01-monitor.exe"
  [ -f "$exe" ] || fail "the build did not make $exe."
  check_no_user "$exe" "the exe"
  grep -q -- "$HOME" "$exe" && fail "the exe contains the home folder path."
  grep -q -- "$ROOT_REL" "$exe" && fail "the exe contains the project folder path."
  echo "No user name or home folder in the exe."

  step "Windows: assembling $folder"
  fresh_folder "$dist" "$folder"
  cp "$exe" "$folder/s01-monitor.exe"
  cp CHANGELOG.md "$folder/CHANGELOG.md"
  ensure_guide
  cp "$GUIDE" "$folder/User-Guide.pdf"
  write_readme "$folder" scripts/windows_readme.txt "Windows build"
  check_texts "$folder"
  check_listing "$folder" "CHANGELOG.md|README.txt|User-Guide.pdf|s01-monitor.exe"

  step "Windows: zipping"
  (cd "$dist" && zip -X -q -r "$(name_for windows).zip" "$(name_for windows)")
  local entries expected
  entries=$(unzip -Z1 "$zip" | grep -v '/$' | C_ORDER | paste -sd'|' -)
  expected="$(name_for windows)/CHANGELOG.md|$(name_for windows)/README.txt|$(name_for windows)/User-Guide.pdf|$(name_for windows)/s01-monitor.exe"
  [ "$entries" = "$expected" ] || fail "the zip holds '$entries', not just the exe, README, guide and CHANGELOG."
  unzip -p "$zip" "$(name_for windows)/s01-monitor.exe" | cmp -s - "$exe" || fail "the exe in the zip is not the one that was built."
  report_zip "$folder" "$zip" "Built on this Mac. It has not been run on Windows."
}

# --- Mac ----------------------------------------------------------------
build_mac() {
  local dist folder zip bin app work
  dist=$(dist_for mac); folder="$dist/$(name_for mac)"; zip="$folder.zip"
  step "Mac: building the app"
  # A separate target folder, so the build with remapped paths does not make
  # your own 'cargo run --release' rebuild everything each time.
  CARGO_TARGET_DIR=target/dist-mac RUSTFLAGS="$REMAP" cargo build --release -p s01-monitor
  bin="target/dist-mac/release/s01-monitor"
  [ -f "$bin" ] || fail "the build did not make $bin."
  check_no_user "$bin" "the program"
  grep -q -- "$HOME" "$bin" && fail "the program contains the home folder path."
  grep -q -- "$ROOT_REL" "$bin" && fail "the program contains the project folder path."
  echo "No user name or home folder in the program."

  step "Mac: making the .app"
  fresh_folder "$dist" "$folder"
  app="$folder/Silence S01.app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  cp "$bin" "$app/Contents/MacOS/s01-monitor"
  chmod 755 "$app/Contents/MacOS/s01-monitor"
  work=$(mktemp -d)
  python3 scripts/make_icon.py "$work/icon.png" 1024
  mkdir "$work/AppIcon.iconset"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" "$work/icon.png" --out "$work/AppIcon.iconset/icon_${size}x${size}.png" >/dev/null
    sips -z "$((size * 2))" "$((size * 2))" "$work/icon.png" --out "$work/AppIcon.iconset/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$work/AppIcon.iconset" -o "$app/Contents/Resources/AppIcon.icns"
  rm -rf "$work"
  cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Silence S01</string>
  <key>CFBundleDisplayName</key><string>Silence S01</string>
  <key>CFBundleIdentifier</key><string>local.silence-s01.monitor</string>
  <key>CFBundleExecutable</key><string>s01-monitor</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
  plutil -lint "$app/Contents/Info.plist" >/dev/null || fail "Info.plist is not valid."
  # An ad-hoc signature: no identity, no name. Apple silicon will not run an
  # unsigned program at all; this is not the same as Apple approving it.
  codesign --force --deep --sign - "$app" 2>&1 | sed '/replacing existing signature/d'
  codesign --verify --deep --strict "$app" || fail "the .app's signature does not verify."
  cp CHANGELOG.md "$folder/CHANGELOG.md"
  ensure_guide
  cp "$GUIDE" "$folder/User-Guide.pdf"
  write_readme "$folder" scripts/mac_readme.txt "Mac build (Apple silicon)"
  check_texts "$folder"
  check_listing "$folder" "CHANGELOG.md|README.txt|Silence S01.app|User-Guide.pdf"
  [ -z "$(find "$folder" -name .DS_Store -o -name '._*')" ] || fail "the folder has Finder leftovers in it."

  step "Mac: zipping"
  # ditto keeps the .app's permissions; the flags leave out the extra
  # metadata files (__MACOSX) and the quarantine mark.
  ditto -c -k --norsrc --noextattr --noqtn --noacl --keepParent "$folder" "$zip"
  local top
  top=$(unzip -Z1 "$zip" | awk -F/ 'NF >= 2 && $2 != "" { print $1 "/" $2 }' | C_ORDER | uniq | paste -sd'|' -)
  local n; n=$(name_for mac)
  [ "$top" = "$n/CHANGELOG.md|$n/README.txt|$n/Silence S01.app|$n/User-Guide.pdf" ] || fail "the zip holds '$top', not just the app, README, guide and CHANGELOG."
  [ "$(unzip -Z1 "$zip" | grep -cE '__MACOSX|\.DS_Store|/\._')" -eq 0 ] || fail "the zip has Finder or metadata leftovers in it."
  # Open the zip as a person would, and check what comes out.
  work=$(mktemp -d)
  ditto -x -k "$zip" "$work"
  codesign --verify --deep --strict "$work/$n/Silence S01.app" || fail "the .app does not verify after unzipping."
  cmp -s "$work/$n/Silence S01.app/Contents/MacOS/s01-monitor" "$app/Contents/MacOS/s01-monitor" || fail "the program in the zip is not the one that was packaged."
  [ -x "$work/$n/Silence S01.app/Contents/MacOS/s01-monitor" ] || fail "the program in the zip is not executable."
  rm -rf "$work"
  report_zip "$folder" "$zip" "Built on this Mac, for Apple silicon only. Not signed by Apple, so macOS asks on first open."
}

# --- ESP32 --------------------------------------------------------------
build_esp() {
  local dist
  dist=$(dist_for esp)
  step "ESP: building the browser page (once, for both)"
  scripts/build_web.sh 2>&1 | tail -n 5 || fail "the browser page did not build."
  for variant in "${ESP_VARIANTS[@]}"; do
    build_esp_variant "$variant" "$dist"
  done
  step "Done: ESP"
  echo "Built, not flashed. Flash the package that matches the hardware; how is in its README.txt."
}

build_esp_variant() { # variant dist
  local variant="$1" dist="$2" folder n
  n=$(esp_name "$variant"); folder="$dist/$n"
  step "ESP ($variant): building the firmware (not flashing)"
  # Each hardware has its own build folder and settings file under firmware/build-release,
  # so these builds never touch the one a person's own idf.py uses.
  # idf.py needs its own environment; a fresh shell keeps it out of this one.
  bash -c 'set +u; . "$1" >/dev/null 2>&1 || { echo "could not load the ESP-IDF environment" >&2; exit 1; }; cd firmware && idf.py -B "build-release/$2" -D "SDKCONFIG=$PWD/build-release/$2/sdkconfig" -D "SDKCONFIG_DEFAULTS=sdkconfig.defaults;sdkconfig.$2.defaults" build' _ "$IDF_EXPORT" "$variant" 2>&1 | tail -n 6 || fail "the firmware did not build."
  local build="firmware/build-release/$variant"
  [ -f "$build/flasher_args.json" ] || fail "the firmware build left no flasher_args.json."

  step "ESP ($variant): checking the image"
  # The three files and where they go. If the layout ever changes, this stops
  # so the README and notes are changed on purpose, not by accident.
  local layout
  layout=$(python3 - "$build" <<'PY'
import json, sys
build = sys.argv[1]
args = json.load(open(build + "/flasher_args.json"))
files = {int(offset, 16): path for offset, path in args["flash_files"].items()}
print(" ".join(f"{offset:#x}={path}" for offset, path in sorted(files.items())))
settings = args["flash_settings"]
print(f'--flash_mode {settings["flash_mode"]} --flash_freq {settings["flash_freq"]} --flash_size {settings["flash_size"]}')
PY
)
  local offsets settings
  offsets=$(echo "$layout" | sed -n 1p); settings=$(echo "$layout" | sed -n 2p)
  [ "$offsets" = "0x1000=bootloader/bootloader.bin 0x8000=partition_table/partition-table.bin 0x20000=s01-monitor-esp32.bin" ] \
    || fail "the flash layout changed ($offsets). Update scripts/release.sh and scripts/esp_readme.txt on purpose."
  local boot="$build/bootloader/bootloader.bin" table="$build/partition_table/partition-table.bin" app="$build/s01-monitor-esp32.bin"
  local size limit=$((0x300000))
  size=$(wc -c < "$app" | tr -d ' ')
  [ "$size" -le "$limit" ] || fail "the firmware is $size bytes; the app partition holds $limit."
  echo "Firmware $size bytes of $limit ($((size * 100 / limit)) %)."
  [ "$(strings -a "$app" | grep -cx "$VERSION")" -gt 0 ] || fail "the firmware does not carry the version $VERSION (it was not rebuilt?)."
  # The image must be for this hardware and no other.
  local other count
  for other in "${ESP_VARIANTS[@]}"; do
    # (Counting, not grep -q: with pipefail an early-exiting grep makes the pipe fail.)
    count=$(strings -a "$app" | grep -cxF "$(esp_board_in "$other")" || true)
    if [ "$other" = "$variant" ]; then
      [ "$count" -gt 0 ] || fail "the $variant image does not name its hardware '$(esp_board_in "$other")'."
    else
      [ "$count" -eq 0 ] || fail "the $variant image names the hardware of the $other image."
    fi
  done
  for file in "$boot" "$table" "$app"; do check_no_user "$file" "$(basename "$file")"; done
  grep -q -- "$ROOT_REL" "$app" && fail "the firmware contains the project folder path."
  # The browser page is inside the image, compressed, so look at the page itself.
  local page_wasm
  page_wasm=$(mktemp)
  gunzip -c web/dist/s01-monitor_bg.wasm.gz > "$page_wasm"
  check_no_user "$page_wasm" "the browser page"
  grep -q -- "$HOME" "$page_wasm" && fail "the browser page contains the home folder path."
  grep -q -- "$ROOT_REL" "$page_wasm" && fail "the browser page contains the project folder path."
  rm -f "$page_wasm"
  echo "Version $VERSION and the right hardware name are in the image; no user name in any file."

  step "ESP ($variant): assembling $folder"
  fresh_folder "$dist" "$folder"
  cp "$boot" "$folder/bootloader.bin"
  cp "$table" "$folder/partition-table.bin"
  cp "$app" "$folder/s01-monitor-esp32.bin"
  printf '%s\n0x1000 bootloader.bin\n0x8000 partition-table.bin\n0x20000 s01-monitor-esp32.bin\n' "$settings" > "$folder/flash_args"
  (cd "$folder" && shasum -a 256 bootloader.bin partition-table.bin s01-monitor-esp32.bin > SHA256SUMS)
  cp CHANGELOG.md "$folder/CHANGELOG.md"
  write_readme "$folder" scripts/esp_readme.txt "$(esp_label "$variant")"
  # The README text is shared; the hardware paragraph is the one that differs.
  sed -e "/@BOARD@/{r scripts/esp_board_$variant.txt" -e 'd' -e '}' "$folder/README.txt" > "$folder/README.tmp" && mv "$folder/README.tmp" "$folder/README.txt"
  grep -q '@BOARD@' "$folder/README.txt" && fail "the README still has its @BOARD@ placeholder."
  check_texts "$folder"
  check_listing "$folder" "CHANGELOG.md|README.txt|SHA256SUMS|bootloader.bin|flash_args|partition-table.bin|s01-monitor-esp32.bin"
  (cd "$folder" && shasum -a 256 -c SHA256SUMS >/dev/null) || fail "the checksums do not match the copied files."

  step "ESP ($variant): zipping"
  local zip="$folder.zip" entries expected f
  (cd "$dist" && zip -X -q -r "$n.zip" "$n")
  entries=$(unzip -Z1 "$zip" | grep -v '/$' | C_ORDER | paste -sd'|' -)
  expected=$(for f in CHANGELOG.md README.txt SHA256SUMS bootloader.bin flash_args partition-table.bin s01-monitor-esp32.bin; do echo "$n/$f"; done | C_ORDER | paste -sd'|' -)
  [ "$entries" = "$expected" ] || fail "the zip holds '$entries', not just the three images, flash_args, SHA256SUMS, README and CHANGELOG."
  for f in bootloader.bin partition-table.bin s01-monitor-esp32.bin; do
    unzip -p "$zip" "$n/$f" | cmp -s - "$folder/$f" || fail "$f in the zip is not the one that was built."
  done
  report_zip "$folder" "$zip" "Built, not flashed. For: $(esp_board_in "$variant")." \
    "the three .bin files, flash_args, SHA256SUMS, README.txt, CHANGELOG.md"
}


report_zip() { # folder zip note [what the zip holds]
  step "Done"
  echo "Folder: $1"
  echo "Zip:    $2 ($(du -h "$2" | cut -f1))"
  echo "SHA-256: $(shasum -a 256 "$2" | cut -d' ' -f1)"
  echo "The zip holds: ${4:-the program, README.txt, User-Guide.pdf, CHANGELOG.md}. Nothing else."
  echo "$3"
}

for t in "${TARGETS[@]}"; do
  "build_$t"
done
