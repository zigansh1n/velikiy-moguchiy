#!/usr/bin/env bash
# Manual check of the unofficial «ChatGPT (ваша подписка)» provider with your own
# ChatGPT sign-in. It talks to OpenAI for real, so CI never runs it.
#
#   scripts/chatgpt-asr-check.sh [audio-file]
#
# Signs in through the browser if this Mac is not signed in yet, sends a second of
# silence, then transcribes `audio-file` (any format ffmpeg reads) if one is given.
# The sign-in is the app's Keychain item; after a sign-in made here, the app may
# ask once for access to it.
set -euo pipefail
cd "$(dirname "$0")/.."

cli=(cargo run --quiet -p velikiy-moguchiy-cli --)

if ! "${cli[@]}" chatgpt status | grep -q '^signed in'; then
  "${cli[@]}" chatgpt login
fi
"${cli[@]}" chatgpt check
if [ "$#" -ge 1 ]; then
  "${cli[@]}" transcribe --provider chatgpt "$1"
fi
