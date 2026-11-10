#!/usr/bin/env bash
# Downloads and caches the Sparkle framework used for in-app updates.
# Output: .sparkle/Sparkle.framework, .sparkle/bin/{sign_update,generate_keys}; deps: curl, tar.

set -euo pipefail

SPARKLE_VERSION="${SPARKLE_VERSION:-2.9.4}"
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CACHE_DIR="$ROOT_DIR/.sparkle"
FRAMEWORK="$CACHE_DIR/Sparkle.framework"
URL="https://github.com/sparkle-project/Sparkle/releases/download/$SPARKLE_VERSION/Sparkle-$SPARKLE_VERSION.tar.xz"

if [[ -d "$FRAMEWORK" && -x "$CACHE_DIR/bin/sign_update" && -x "$CACHE_DIR/bin/generate_keys" ]]; then
  printf 'Sparkle %s already cached at %s\n' "$SPARKLE_VERSION" "$CACHE_DIR"
  exit 0
fi

rm -rf "$CACHE_DIR"
mkdir -p "$CACHE_DIR"
printf 'Downloading Sparkle %s…\n' "$SPARKLE_VERSION"
curl -fsSL "$URL" | tar -xJ -C "$CACHE_DIR"

if [[ ! -d "$FRAMEWORK" ]]; then
  printf 'ERROR: Sparkle.framework missing from %s\n' "$URL" >&2
  exit 1
fi

printf 'Sparkle %s ready:\n  %s\n  %s\n' \
  "$SPARKLE_VERSION" "$FRAMEWORK" "$CACHE_DIR/bin/sign_update"
