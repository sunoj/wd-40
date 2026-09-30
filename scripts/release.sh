#!/usr/bin/env bash
# Builds, signs, notarizes and publishes one Sparkle release of WD-40.
# Usage: ./scripts/release.sh <version> [release notes].
# Secrets live in the login keychain: the upload secret, Sparkle's signing key,
# and notarytool's credentials. Nothing unnotarized can be published from here.

set -euo pipefail

VERSION="${1:?usage: release.sh <version> [notes]}"
NOTES="${2:-Bug fixes and improvements.}"

# The Sparkle signing key already lives in the login keychain; the relay's
# upload secret belongs beside it rather than in a shell that keeps history.
KEYCHAIN_SERVICE="wd40-release"
if [ -z "${UPLOAD_SECRET:-}" ]; then
  UPLOAD_SECRET="$(security find-generic-password -s "$KEYCHAIN_SERVICE" -a UPLOAD_SECRET -w 2>/dev/null || true)"
fi
if [ -z "${UPLOAD_SECRET:-}" ]; then
  echo "No upload secret. Store one in the login keychain:" >&2
  echo "  security add-generic-password -U -s $KEYCHAIN_SERVICE -a UPLOAD_SECRET -w" >&2
  echo "(or set UPLOAD_SECRET for this run)" >&2
  exit 1
fi

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

RELAY="https://wd40-release.sunoj-mings.workers.dev"
SIGN_UPDATE=".sparkle/bin/sign_update"
MIN_OS="13.0"
BUILD="$(date +%Y%m%d%H%M)"
OUT_DIR="dist"
ZIP="wd40-$VERSION.zip"
DMG="wd40-$VERSION.dmg"
APP="dist/WD-40.app"

CARGO_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
if [[ "$CARGO_VERSION" != "$VERSION" ]]; then
  printf 'ERROR: Cargo.toml is at %s but you asked to release %s\n' "$CARGO_VERSION" "$VERSION" >&2
  exit 1
fi

# An ad-hoc build has no business on the relay: Gatekeeper rejects it, and the
# only thing it can teach a person is to click through the warning. So the
# identity is resolved before anything is built rather than defaulted away.
NOTARY_PROFILE="${NOTARY_PROFILE:-wd40-notary}"
if [ -z "${SIGN_IDENTITY:-}" ]; then
  IDENTITIES="$(security find-identity -v -p codesigning |
    sed -n 's/^ *[0-9]*) *[0-9A-F]* "\(Developer ID Application:.*\)"$/\1/p')"
  if [ "$(printf '%s\n' "$IDENTITIES" | grep -c .)" != "1" ]; then
    printf 'ERROR: need exactly one Developer ID Application identity, found:\n' >&2
    printf '%s\n' "${IDENTITIES:-  (none)}" >&2
    printf 'Set SIGN_IDENTITY to choose one.\n' >&2
    exit 1
  fi
  SIGN_IDENTITY="$IDENTITIES"
fi
printf 'Signing as %s\n' "$SIGN_IDENTITY"

# Apple reads a zip of the app; the ticket goes onto the app itself, so both the
# Sparkle zip and the disk image below are cut from an already-stapled copy.
# A submission Apple never finishes would otherwise hold the release open for as
# long as the terminal stays awake, so the wait is bounded. The service keeps
# processing after the timeout; only our waiting stops.
notarize() {
  local submit="$1" staple="$2"
  if ! xcrun notarytool submit "$submit" \
    --keychain-profile "$NOTARY_PROFILE" --wait --timeout 30m; then
    printf 'ERROR: notarization failed for %s.\n' "$submit" >&2
    printf 'If the profile is missing, store it once:\n' >&2
    printf '  xcrun notarytool store-credentials %s \\\n' "$NOTARY_PROFILE" >&2
    printf '    --apple-id <apple-id> --team-id <team-id> --password <app-specific-password>\n' >&2
    exit 1
  fi
  xcrun stapler staple "$staple"
}

SIGN_IDENTITY="$SIGN_IDENTITY" WD40_VERSION="$VERSION" WD40_BUILD="$BUILD" \
  ./scripts/bundle.sh >/dev/null

printf 'Notarizing the app (profile: %s)…\n' "$NOTARY_PROFILE"
/usr/bin/ditto -c -k --keepParent "$APP" "$OUT_DIR/notarize-$VERSION.zip"
notarize "$OUT_DIR/notarize-$VERSION.zip" "$APP"
rm -f "$OUT_DIR/notarize-$VERSION.zip"

# Sparkle updates from the zip. People download the disk image: a zip lands in
# ~/Downloads and gets run from there, and a quarantined app run in place is
# translocated to a read-only path where Sparkle cannot replace it. The
# Applications symlink is what makes dragging it out the obvious move.
/usr/bin/ditto -c -k --keepParent "$APP" "$OUT_DIR/$ZIP"

STAGE="$OUT_DIR/dmg-$VERSION"
rm -rf "$STAGE" "$OUT_DIR/$DMG"
mkdir -p "$STAGE"
/usr/bin/ditto "$APP" "$STAGE/WD-40.app"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "WD-40 $VERSION" -srcfolder "$STAGE" -ov -format UDZO \
  -quiet "$OUT_DIR/$DMG"
rm -rf "$STAGE"

# The disk image is the file people download, so Gatekeeper judges it on its own
# signature before it ever looks at the app inside. Stapling it too means the
# first open works on a machine that cannot reach Apple.
codesign --force --sign "$SIGN_IDENTITY" --timestamp "$OUT_DIR/$DMG"
printf 'Notarizing the disk image…\n'
notarize "$OUT_DIR/$DMG" "$OUT_DIR/$DMG"

SIG_LINE="$("$SIGN_UPDATE" "$OUT_DIR/$ZIP")"
LENGTH="$(printf '%s' "$SIG_LINE" | sed -n 's/.*length="\([0-9]*\)".*/\1/p')"

# An enclosure without a signature, or with a length that describes some other
# file, is well-formed XML that every installed copy will refuse to update from.
# xmllint below cannot see either, so they are checked here.
SIGNATURE="$(printf '%s' "$SIG_LINE" | sed -n 's/.*sparkle:edSignature="\([^"]*\)".*/\1/p')"
if [[ -z "$SIGNATURE" ]]; then
  printf 'ERROR: sign_update produced no signature; nothing uploaded\n' >&2
  exit 1
fi
ZIP_BYTES="$(/usr/bin/stat -f%z "$OUT_DIR/$ZIP")"
if [[ "$LENGTH" != "$ZIP_BYTES" ]]; then
  printf 'ERROR: appcast says %s bytes, the zip is %s; nothing uploaded\n' \
    "${LENGTH:-<none>}" "$ZIP_BYTES" >&2
  exit 1
fi

# A "]]>" inside the notes would close the CDATA section early and let the rest
# of the text be parsed as markup. Split the terminator across two sections.
NOTES_XML="${NOTES//]]>/]]]]><![CDATA[>}"

cat > "$OUT_DIR/appcast.xml" <<XML
<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>WD-40</title>
    <item>
      <title>Version $VERSION</title>
      <sparkle:shortVersionString>$VERSION</sparkle:shortVersionString>
      <sparkle:version>$BUILD</sparkle:version>
      <sparkle:minimumSystemVersion>$MIN_OS</sparkle:minimumSystemVersion>
      <description><![CDATA[ $NOTES_XML ]]></description>
      <enclosure url="$RELAY/$ZIP" $SIG_LINE type="application/zip" />
    </item>
  </channel>
</rss>
XML

# Never publish a feed that Sparkle cannot parse: a broken appcast breaks
# updates for every installed copy, and the upload is not transactional.
if ! xmllint --noout "$OUT_DIR/appcast.xml" 2>/dev/null; then
  printf 'ERROR: generated appcast.xml is not well-formed XML; nothing uploaded\n' >&2
  exit 1
fi

# The last gate, and the only one that reads the artifacts themselves rather
# than the build directory they came from. 0.6.0 went out ad-hoc signed because
# nothing here looked; the uploads are not transactional, so a bad one is live.
./scripts/verify-release.sh "$OUT_DIR/$ZIP" "$OUT_DIR/$DMG"

# Both archives are cut from this stapled app, so its tree is what they carry.
# The credential scanner reads directories, not archive bytes.
GUARD="${SECURITY_GUARD:-$HOME/.local/bin/security-guard}"
if [[ ! -x "$GUARD" ]]; then
  printf 'ERROR: credential scanner %s not found; nothing uploaded\n' "$GUARD" >&2
  exit 1
fi
"$GUARD" artifact "$ROOT_DIR/$APP" --repo "$ROOT_DIR"

curl -fsS -X PUT "$RELAY/$ZIP" \
  -H "authorization: Bearer $UPLOAD_SECRET" \
  -H "content-type: application/zip" \
  --data-binary "@$OUT_DIR/$ZIP" >/dev/null
curl -fsS -X PUT "$RELAY/$DMG" \
  -H "authorization: Bearer $UPLOAD_SECRET" \
  -H "content-type: application/x-apple-diskimage" \
  --data-binary "@$OUT_DIR/$DMG" >/dev/null
# The feed goes last: it is the only file that makes the others discoverable,
# so publishing it before them would advertise a build nobody can fetch.
curl -fsS -X PUT "$RELAY/appcast.xml" \
  -H "authorization: Bearer $UPLOAD_SECRET" \
  -H "content-type: application/xml" \
  --data-binary "@$OUT_DIR/appcast.xml" >/dev/null

printf 'Published WD-40 %s (build %s, %s bytes)\n' "$VERSION" "$BUILD" "$LENGTH"
printf '  feed: %s/appcast.xml\n  zip:  %s/%s\n  dmg:  %s/%s\n' "$RELAY" "$RELAY" "$ZIP" "$RELAY" "$DMG"
