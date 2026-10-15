#!/bin/bash
# Package VelikiyMoguchiy.app for release: embed velikiy-moguchiy-cli (and mic-warm),
# codesign, optionally notarize, and zip.
#
# Usage: package-app.sh <app-path> <cli-path> <output-zip> [<mic-warm-path>]
#
# Behavior is controlled by environment variables:
#   CODESIGN_IDENTITY                        Identity used for signing (name or SHA-1).
#                                            Falls back to ad-hoc signing when unset
#                                            (builds without the signing secrets, e.g. forks).
#   CODESIGN_LEAF_SHA1                       SHA-1 of a self-signed CODESIGN_IDENTITY: every
#                                            piece of code gets a designated requirement
#                                            pinning this certificate, so macOS keeps the
#                                            privacy permissions across builds; no timestamp.
#   CODESIGN_KEYCHAIN                        Keychain that holds CODESIGN_IDENTITY.
#   APPLE_ID, APPLE_APP_PASSWORD,
#   APPLE_TEAM_ID                            When all set (and CODESIGN_IDENTITY is set),
#                                            the app is notarized and stapled before zipping.
set -euo pipefail

APP_PATH="$1"
CLI_PATH="$2"
OUTPUT_ZIP="$3"
MIC_WARM_PATH="${4:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENTITLEMENTS="$SCRIPT_DIR/../MacApp/VelikiyMoguchiy/VelikiyMoguchiy.entitlements"

test -d "$APP_PATH"
test -x "$CLI_PATH"
test -f "$ENTITLEMENTS"

cp "$CLI_PATH" "$APP_PATH/Contents/MacOS/velikiy-moguchiy-cli"
chmod +x "$APP_PATH/Contents/MacOS/velikiy-moguchiy-cli"
if [ -n "$MIC_WARM_PATH" ]; then
  test -x "$MIC_WARM_PATH"
  cp "$MIC_WARM_PATH" "$APP_PATH/Contents/MacOS/mic-warm"
  chmod +x "$APP_PATH/Contents/MacOS/mic-warm"
fi

if [ -n "${CODESIGN_IDENTITY:-}" ]; then
  echo "Signing with identity: $CODESIGN_IDENTITY"
  SIGN_FLAGS=(--force --options runtime --sign "$CODESIGN_IDENTITY")
  if [ -n "${CODESIGN_KEYCHAIN:-}" ]; then SIGN_FLAGS+=(--keychain "$CODESIGN_KEYCHAIN"); fi
  if [ -n "${CODESIGN_LEAF_SHA1:-}" ]; then SIGN_FLAGS+=(--timestamp=none); else SIGN_FLAGS+=(--timestamp); fi

  # sign <path> <identifier> [codesign options]. The default requirement of a
  # self-signed certificate is not guaranteed to stay the same, and TCC compares
  # it on every launch, so it is written out.
  sign() {
    local path="$1" ident="$2"
    shift 2
    local req=()
    if [ -n "${CODESIGN_LEAF_SHA1:-}" ]; then
      req=(--identifier "$ident" "-r=designated => identifier \"$ident\" and certificate leaf = H\"$CODESIGN_LEAF_SHA1\"")
    fi
    codesign "${SIGN_FLAGS[@]}" ${req[@]+"${req[@]}"} "$@" "$path"
  }

  # Sign nested code first (frameworks and dylibs, if any), then embedded
  # helper binaries, then the outer bundle. The entitlements are applied to
  # every executable that may capture audio under the hardened runtime.
  while IFS= read -r -d '' nested; do
    if [ -d "$nested" ]; then
      ident=$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$nested/Resources/Info.plist")
    else
      ident=$(basename "$nested" .dylib)
    fi
    sign "$nested" "$ident"
  done < <(find "$APP_PATH/Contents" -depth \( -name "*.dylib" -o -name "*.framework" \) -print0)

  sign "$APP_PATH/Contents/MacOS/velikiy-moguchiy-cli" velikiy-moguchiy-cli --entitlements "$ENTITLEMENTS"
  if [ -f "$APP_PATH/Contents/MacOS/mic-warm" ]; then
    sign "$APP_PATH/Contents/MacOS/mic-warm" mic-warm --entitlements "$ENTITLEMENTS"
  fi
  sign "$APP_PATH" "$(/usr/libexec/PlistBuddy -c "Print :CFBundleIdentifier" "$APP_PATH/Contents/Info.plist")" \
    --entitlements "$ENTITLEMENTS"
else
  echo "CODESIGN_IDENTITY not set; using ad-hoc signature"
  codesign --force --deep --sign - "$APP_PATH"
fi

codesign --verify --deep --strict --verbose=2 "$APP_PATH"
if [ -n "${CODESIGN_LEAF_SHA1:-}" ]; then
  codesign --verify --deep --strict -R="certificate leaf = H\"$CODESIGN_LEAF_SHA1\"" "$APP_PATH"
fi
codesign -d -r- "$APP_PATH"
codesign -dvv "$APP_PATH" 2>&1 | grep -E '^(Identifier|Authority|CodeDirectory)'

if [ -n "${CODESIGN_IDENTITY:-}" ] && [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ]; then
  echo "Submitting $APP_PATH for notarization"
  NOTARIZE_ZIP="$(mktemp -d)/notarize.zip"
  ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$NOTARIZE_ZIP"

  SUBMIT_JSON=$(xcrun notarytool submit "$NOTARIZE_ZIP" \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_APP_PASSWORD" \
    --team-id "$APPLE_TEAM_ID" \
    --wait --timeout 30m \
    --output-format json)
  rm -f "$NOTARIZE_ZIP"
  echo "$SUBMIT_JSON"

  SUBMISSION_ID=$(echo "$SUBMIT_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
  STATUS=$(echo "$SUBMIT_JSON" | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])')
  if [ "$STATUS" != "Accepted" ]; then
    echo "Notarization failed with status: $STATUS" >&2
    xcrun notarytool log "$SUBMISSION_ID" \
      --apple-id "$APPLE_ID" \
      --password "$APPLE_APP_PASSWORD" \
      --team-id "$APPLE_TEAM_ID" >&2 || true
    exit 1
  fi

  xcrun stapler staple "$APP_PATH"
  xcrun stapler validate "$APP_PATH"
  spctl --assess --type execute --verbose=2 "$APP_PATH"
elif [ -n "${CODESIGN_IDENTITY:-}" ] && [ -z "${CODESIGN_LEAF_SHA1:-}" ]; then
  echo "Notarization credentials not set; skipping notarization" >&2
fi

ditto -c -k --sequesterRsrc --keepParent "$APP_PATH" "$OUTPUT_ZIP"
echo "Packaged $OUTPUT_ZIP"
