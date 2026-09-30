#!/usr/bin/env bash
# Builds WD-40.app: Rust binary + Sparkle framework + signed bundle.
# Usage: [WD40_VERSION=x.y.z] [WD40_BUILD=n] [SIGN_IDENTITY=...] ./scripts/bundle.sh
# Output: dist/WD-40.app; deps: cargo, scripts/fetch-sparkle.sh, codesign.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

VERSION="${WD40_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
# Sparkle orders builds by this number, not by VERSION. A local build numbered
# 1 is older than every published one, so the feed offered it a downgrade.
BUILD="${WD40_BUILD:-$(date +%Y%m%d%H%M)}"
APP_DIR="dist/WD-40.app"
CONTENTS="$APP_DIR/Contents"
CARGO_OUT="${CARGO_TARGET_DIR:-target}/release"

./scripts/fetch-sparkle.sh
cargo build --release

rm -rf "$APP_DIR"
mkdir -p "$CONTENTS/MacOS" "$CONTENTS/Frameworks" "$CONTENTS/Resources"
cp "$CARGO_OUT/wd40-menu" "$CONTENTS/MacOS/wd40-menu"
cp "$CARGO_OUT/wd40" "$CONTENTS/Resources/wd40"
cp -R .sparkle/Sparkle.framework "$CONTENTS/Frameworks/"
cp AppIcon.icns "$CONTENTS/Resources/AppIcon.icns"

sed -e "s/__VERSION__/$VERSION/" -e "s/__BUILD__/$BUILD/" Info.plist > "$CONTENTS/Info.plist"

# Sparkle lives in Contents/Frameworks; the binary needs an rpath to find it.
install_name_tool -add_rpath "@executable_path/../Frameworks" \
  "$CONTENTS/MacOS/wd40-menu" 2>/dev/null || true

SIGN_ID="${SIGN_IDENTITY:--}"
CODESIGN_OPTIONS=(--force --sign "$SIGN_ID")
[[ "$SIGN_ID" != "-" ]] && CODESIGN_OPTIONS+=(--options runtime --timestamp)

sign_one() {
  if [[ "$SIGN_ID" = "-" ]]; then
    codesign "${CODESIGN_OPTIONS[@]}" "$1" >/dev/null 2>&1 || true
  else
    codesign "${CODESIGN_OPTIONS[@]}" "$1"
  fi
}

# Nested Sparkle helpers must be signed before the framework that contains them.
find "$CONTENTS/Frameworks/Sparkle.framework" \
  \( -name "*.xpc" -o -name "*.app" -o -name "Autoupdate" \) -print0 2>/dev/null |
  while IFS= read -r -d '' nested; do sign_one "$nested"; done
sign_one "$CONTENTS/Frameworks/Sparkle.framework"
sign_one "$CONTENTS/Resources/wd40"
sign_one "$CONTENTS/MacOS/wd40-menu"
sign_one "$APP_DIR"

printf 'Built %s (v%s build %s)\n' "$APP_DIR" "$VERSION" "$BUILD"
