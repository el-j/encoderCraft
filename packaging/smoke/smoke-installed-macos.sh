#!/usr/bin/env bash
set -euo pipefail

dmg="${1:?DMG path}"
cli_archive="${2:?CLI archive path}"
version="${3:?expected version}"
fixture="${4:?fixture path}"
work="$(mktemp -d)"
mountpoint="$work/mount"
mounted=0

cleanup() {
  if [[ "$mounted" = 1 ]]; then
    hdiutil detach "$mountpoint" -quiet
  fi
  rm -rf "$work"
}
trap cleanup EXIT

mkdir -p "$mountpoint" "$work/Applications"
hdiutil attach -nobrowse -readonly -mountpoint "$mountpoint" "$dmg"
mounted=1
ditto "$mountpoint/EncoderCraft.app" "$work/Applications/EncoderCraft.app"
app="$work/Applications/EncoderCraft.app"
gui="$app/Contents/MacOS/EncoderCraft"

test "$("$gui" --version)" = "encodercraft $version"
for license in LICENSE-MIT LICENSE-APACHE NOTICE; do
  test -s "$app/Contents/Resources/$license"
done

mkdir "$work/cli"
unzip -q "$cli_archive" -d "$work/cli"
cli="$(find "$work/cli" -type f -name encodercraft-cli -print -quit)"
test -n "$cli"
chmod +x "$cli"
test "$("$cli" --version)" = "encodercraft-cli $version"

"$cli" encode "$fixture" -p "Still Image PNG (Lossless)" -o "$work/out.png" >/dev/null
test -s "$work/out.png"
"$cli" encode "$fixture" -o "$work/out.mp4" >/dev/null
test "$(wc -c < "$work/out.mp4")" -gt 1000

echo "OK: installed DMG and CLI package ($version)"
