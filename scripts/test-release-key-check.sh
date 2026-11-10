#!/usr/bin/env bash
# Runs release.sh in a stubbed tree to prove it stops before building when the
# keychain's Sparkle key is not the SUPublicEDKey the app trusts.
# Usage: ./scripts/test-release-key-check.sh (no network, no keychain, no build).

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TRUSTED="RU1CL2LFNZs4AhiBQ5ZoHelifYyJ5yv2I7rEi7mSahM="
OTHER="PclJNaSUSridNLOXbtKGRTGgXWtlw2dOGbQ0Acq7XiM="
FAILURES=0

# One throwaway checkout: the real release scripts, stubbed tools around them.
make_tree() {
  local dir="$1" plist_key="$2" keychain_key="$3"
  mkdir -p "$dir/scripts" "$dir/.sparkle/bin"
  cp "$ROOT_DIR/scripts/release.sh" "$ROOT_DIR/scripts/check-sparkle-key.sh" "$dir/scripts/"
  printf '[package]\nversion = "9.9.9"\n' > "$dir/Cargo.toml"
  /usr/libexec/PlistBuddy -c 'Add :CFBundleName string WD-40' "$dir/Info.plist" >/dev/null
  if [[ -n "$plist_key" ]]; then
    /usr/libexec/PlistBuddy -c "Add :SUPublicEDKey string $plist_key" "$dir/Info.plist" >/dev/null
  fi
  # shellcheck disable=SC2016 # $1 belongs to the stub, not to this shell
  if [[ "$keychain_key" == none ]]; then
    : # a fresh checkout before Sparkle's tools exist
  elif [[ -n "$keychain_key" ]]; then
    printf '#!/bin/sh\n[ "$1" = -p ] && echo %s\n' "$keychain_key" > "$dir/.sparkle/bin/generate_keys"
  else
    printf '#!/bin/sh\necho "ERROR: No existing signing key found!"; exit 1\n' > "$dir/.sparkle/bin/generate_keys"
  fi
  # Reaching the build is the observable effect; stop there. The fetch stub
  # records that it ran, so the order of fetch and check is observable too.
  printf '#!/bin/sh\ntouch "%s/built"\nexit 3\n' "$dir" > "$dir/scripts/bundle.sh"
  printf '#!/bin/sh\ntouch "%s/fetched"\n' "$dir" > "$dir/scripts/fetch-sparkle.sh"
  chmod +x "$dir/scripts/"*.sh
  [[ -e "$dir/.sparkle/bin/generate_keys" ]] && chmod +x "$dir/.sparkle/bin/generate_keys"
  return 0
}

# expect <name> <plist key> <keychain key> <built|stopped> [stderr fragment]
expect() {
  local name="$1" want="$4" fragment="${5:-}" dir
  dir="$(mktemp -d)"
  make_tree "$dir" "$2" "$3"
  UPLOAD_SECRET=stub SIGN_IDENTITY=stub "$dir/scripts/release.sh" 9.9.9 \
    >"$dir/out" 2>"$dir/err" || true
  local got="stopped"
  [[ -e "$dir/built" ]] && got="built"
  [[ -e "$dir/fetched" ]] || got="$got without fetching Sparkle"
  if [[ "$got" != "$want" ]] || { [[ -n "$fragment" ]] && ! grep -q "$fragment" "$dir/err"; }; then
    printf 'FAIL  %s: wanted %s, got %s\n' "$name" "$want" "$got"
    sed 's/^/      /' "$dir/err"
    FAILURES=$((FAILURES + 1))
  else
    printf 'ok    %s\n' "$name"
  fi
  rm -rf "$dir"
}

expect "matching key reaches the build" "$TRUSTED" "$TRUSTED" built
expect "other key stops before the build" "$TRUSTED" "$OTHER" stopped "the app trusts $TRUSTED"
expect "missing keychain key stops" "$TRUSTED" "" stopped "no Sparkle signing key"
expect "absent generate_keys names the tool" "$TRUSTED" none stopped "not found or not executable"
expect "plist without SUPublicEDKey stops" "" "$TRUSTED" stopped "has no SUPublicEDKey"

[[ "$FAILURES" -eq 0 ]] && printf '\nAll checks passed.\n' || exit 1
