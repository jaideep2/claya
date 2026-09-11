#!/usr/bin/env bash
# Build, sign, notarize and staple claya.
#
#   ./scripts/release.sh --adhoc     verify the hardened runtime locally (no Apple account)
#   ./scripts/release.sh             full Developer ID sign + notarize + staple
#
# Full mode expects:
#   SIGNING_IDENTITY   "Developer ID Application: Your Name (TEAMID)"
#   APPLE_ID           your Apple ID email
#   APPLE_TEAM_ID      10-character team id
#   APPLE_PASSWORD     an app-specific password (appleid.apple.com), NOT your login
set -euo pipefail
cd "$(dirname "$0")/.."

ADHOC=0
[[ "${1:-}" == "--adhoc" ]] && ADHOC=1

APP="src-tauri/target/release/bundle/macos/Claya.app"
ENTITLEMENTS="src-tauri/entitlements.plist"

# The updater public key is compiled into every binary. Shipping with the
# placeholder means no update can ever be verified against YOUR private key —
# and it is silent, because a wrong key looks exactly like "no update available".
PUBKEY=$(python3 -c "import json;print(json.load(open('src-tauri/tauri.conf.json'))['plugins']['updater']['pubkey'])")
if [[ -f src-tauri/.placeholder-pubkey ]] && [[ "$PUBKEY" == "$(cat src-tauri/.placeholder-pubkey)" ]]; then
  cat <<'MSG' >&2
refusing to build: the updater public key is still the placeholder.

  1. npx tauri signer generate -w ~/.tauri/claya.key
  2. paste the .pub contents into plugins.updater.pubkey in tauri.conf.json
  3. rm src-tauri/.placeholder-pubkey
  4. export TAURI_SIGNING_PRIVATE_KEY=~/.tauri/claya.key

Back the private key up offline first. Losing it means no update can ever reach
an existing install again.
MSG
  exit 1
fi

echo "==> building"
npm run tauri build

if [[ $ADHOC -eq 1 ]]; then
  IDENTITY="-"
  echo "==> ad-hoc signing with hardened runtime (local verification only)"
else
  IDENTITY="${SIGNING_IDENTITY:?set SIGNING_IDENTITY, or pass --adhoc}"
  echo "==> signing as $IDENTITY"
fi

# --options runtime is the hardened runtime. This is the flag that would break the
# app if runtime code evaluation needed a JIT entitlement — which is exactly what
# --adhoc exists to check, before an Apple account is anywhere near the process.
codesign --force --deep --timestamp${ADHOC:+} \
  --options runtime \
  --entitlements "$ENTITLEMENTS" \
  --sign "$IDENTITY" \
  "$APP"

echo "==> verifying signature"
codesign --verify --strict --verbose=2 "$APP"
echo "--- hardened runtime flags ---"
codesign --display --verbose=2 "$APP" 2>&1 | grep -E "flags|Identifier" || true

if [[ $ADHOC -eq 1 ]]; then
  echo
  echo "Ad-hoc signed. Launch it and confirm the canvas still mounts:"
  echo "  $APP/Contents/MacOS/Claya"
  echo "An ad-hoc signature cannot be notarized and is not distributable."
  exit 0
fi

echo "==> notarizing"
ZIP="src-tauri/target/release/bundle/macos/Claya.zip"
ditto -c -k --keepParent "$APP" "$ZIP"
xcrun notarytool submit "$ZIP" \
  --apple-id "${APPLE_ID:?}" \
  --team-id "${APPLE_TEAM_ID:?}" \
  --password "${APPLE_PASSWORD:?}" \
  --wait

echo "==> stapling"
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"

echo "==> gatekeeper assessment"
spctl --assess --type execute --verbose=4 "$APP"

echo
echo "Done. Distributable bundle: $APP"
