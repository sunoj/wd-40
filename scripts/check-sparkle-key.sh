#!/usr/bin/env bash
# Fails unless the keychain's Sparkle signing key is the one the app trusts.
# Usage: check-sparkle-key.sh <Info.plist> <generate_keys>
# A different key signs a well-formed update that every installed copy refuses.

set -euo pipefail

PLIST="${1:?usage: check-sparkle-key.sh <Info.plist> <generate_keys>}"
GENERATE_KEYS="${2:?usage: check-sparkle-key.sh <Info.plist> <generate_keys>}"

if [[ ! -f "$PLIST" ]]; then
  printf 'ERROR: %s not found\n' "$PLIST" >&2
  exit 1
fi
EXPECTED="$(/usr/libexec/PlistBuddy -c 'Print :SUPublicEDKey' "$PLIST" 2>/dev/null || true)"
if [[ -z "$EXPECTED" ]]; then
  printf 'ERROR: %s has no SUPublicEDKey\n' "$PLIST" >&2
  exit 1
fi

if [[ ! -x "$GENERATE_KEYS" ]]; then
  printf 'ERROR: %s not found or not executable; run scripts/fetch-sparkle.sh\n' "$GENERATE_KEYS" >&2
  exit 1
fi
# -p only prints the public half of the key already in the keychain.
if ! ACTUAL="$("$GENERATE_KEYS" -p 2>/dev/null | tail -1)" || [[ -z "$ACTUAL" ]]; then
  printf 'ERROR: no Sparkle signing key in the keychain; the app trusts %s\n' "$EXPECTED" >&2
  exit 1
fi

if [[ "$ACTUAL" != "$EXPECTED" ]]; then
  printf 'ERROR: the keychain signs with %s but the app trusts %s.\n' "$ACTUAL" "$EXPECTED" >&2
  printf 'Every installed copy would refuse this update. Release from the Mac that\n' >&2
  printf 'holds the trusted key, or move it here with generate_keys -x / -f.\n' >&2
  printf 'Never generate a new key: installed copies only trust the old one.\n' >&2
  exit 1
fi
printf 'Sparkle key matches Info.plist (%s)\n' "$EXPECTED"
