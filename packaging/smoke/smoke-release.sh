#!/usr/bin/env bash
# Installs nothing: unpacks a release archive and proves it is usable.
#   smoke-release.sh <archive.tar.gz|archive.zip> <expected-version>
# Checks: the CLI reports the expected version, converts a picture to PNG and to a video that is
# not empty, and (Linux tarballs) the desktop entry, metainfo, icons and licences are bundled.
set -euo pipefail

archive="${1:?archive path}"
version="${2:?expected version}"
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

case "$archive" in
  *.tar.gz) tar -xzf "$archive" -C "$work" ;;
  *.zip) unzip -q "$archive" -d "$work" ;;
  *) echo "unsupported archive: $archive" >&2; exit 2 ;;
esac

cli="$(find "$work" -type f \( -name encodercraft-cli -o -name encodercraft-cli.exe \) | head -n 1)"
[ -n "$cli" ] || { echo "FAIL: no encodercraft-cli in $archive" >&2; exit 1; }
chmod +x "$cli"
root="$(dirname "$cli")"

reported="$("$cli" --version)"
[ "$reported" = "encodercraft-cli $version" ] || { echo "FAIL: version '$reported', expected '$version'" >&2; exit 1; }

"$cli" encode "$here/smoke.bmp" -p "Still Image PNG (Lossless)" -o "$work/out.png" >/dev/null
[ -s "$work/out.png" ] || { echo "FAIL: PNG output missing or empty" >&2; exit 1; }
"$cli" probe "$work/out.png" >/dev/null

"$cli" encode "$here/smoke.bmp" -o "$work/out.mp4" >/dev/null
[ "$(wc -c < "$work/out.mp4")" -gt 1000 ] || { echo "FAIL: MP4 output is empty" >&2; exit 1; }

gui="$(find "$work" -type f \( -name encodercraft -o -name encodercraft.exe \) | head -n 1)"
[ -n "$gui" ] || echo "note: archive has no GUI binary (CLI-only package)"

for lic in LICENSE-MIT LICENSE-APACHE NOTICE; do
  find "$work" -type f -name "$lic" | grep -q . || { echo "FAIL: $lic not bundled" >&2; exit 1; }
done

case "$archive" in
  *linux*.tar.gz)
    pkg="$(dirname "$root")"
    for f in share/applications/ai.storyteller.encodercraft.desktop \
             share/metainfo/ai.storyteller.encodercraft.metainfo.xml \
             share/icons/hicolor/256x256/apps/ai.storyteller.encodercraft.png; do
      [ -s "$pkg/$f" ] || { echo "FAIL: $f missing" >&2; exit 1; }
    done ;;
esac

echo "OK: $archive ($version)"
