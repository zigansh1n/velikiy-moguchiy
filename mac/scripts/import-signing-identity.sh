#!/bin/bash
# Import the project's release signing identity into a new keychain for package-app.sh.
#
# Usage: import-signing-identity.sh <keychain-path>
#
# Reads MAC_SIGNING_P12_BASE64 and MAC_SIGNING_P12_PASSWORD. The identity is the
# self-signed certificate in release-signing-certificate.crt: macOS keeps
# Accessibility, Input Monitoring and Microphone grants for as long as every build
# is signed with it, so a .p12 with any other certificate is refused instead of
# silently resetting the grants of every user.
#
# Prints the certificate's SHA-1, which package-app.sh takes as CODESIGN_IDENTITY
# and CODESIGN_LEAF_SHA1 (with CODESIGN_KEYCHAIN set to the keychain path).
set -euo pipefail

KEYCHAIN="$1"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CERT="$SCRIPT_DIR/release-signing-certificate.crt"

: "${MAC_SIGNING_P12_BASE64:?}"
: "${MAC_SIGNING_P12_PASSWORD:?}"

EXPECTED=$(openssl x509 -in "$CERT" -noout -fingerprint -sha1 | sed 's/.*=//; s/://g' | tr 'a-f' 'A-F')

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
echo "$MAC_SIGNING_P12_BASE64" | base64 --decode > "$WORK/identity.p12"

KEYCHAIN_PASSWORD=$(openssl rand -hex 24)
security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security set-keychain-settings -lut 3600 "$KEYCHAIN"
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
security import "$WORK/identity.p12" -k "$KEYCHAIN" -f pkcs12 -P "$MAC_SIGNING_P12_PASSWORD" \
  -T /usr/bin/codesign >/dev/null
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$KEYCHAIN_PASSWORD" "$KEYCHAIN" >/dev/null

# Without -v: the certificate is not trusted on the runner, and codesign does not need it to be.
if ! security find-identity -p codesigning "$KEYCHAIN" | grep -q " $EXPECTED "; then
  echo "::error::MAC_SIGNING_P12_BASE64 does not hold the certificate in mac/scripts/release-signing-certificate.crt" >&2
  exit 1
fi
echo "$EXPECTED"
