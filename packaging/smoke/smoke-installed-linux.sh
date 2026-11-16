#!/usr/bin/env bash
set -euo pipefail

kind="${1:?expected deb, rpm, or appimage}"
package="${2:?package path}"
version="${3:?expected version}"
fixture="${4:?fixture path}"
package="$(realpath "$package")"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

case "$kind" in
  deb)
    sudo apt-get update
    sudo apt-get install -y "$package"
    cli=/usr/bin/encodercraft-cli
    gui=/usr/bin/encodercraft
    docs=/usr/share/doc/encodercraft
    share=/usr/share
    ;;
  rpm)
    sudo apt-get update
    sudo apt-get install -y rpm
    sudo rpm -Uvh --nodeps "$package"
    cli=/usr/bin/encodercraft-cli
    gui=/usr/bin/encodercraft
    docs=/usr/share/doc/encodercraft
    share=/usr/share
    ;;
  appimage)
    chmod +x "$package"
    (cd "$work" && "$package" --appimage-extract >/dev/null)
    cli="$work/squashfs-root/usr/bin/encodercraft-cli"
    gui="$work/squashfs-root/usr/bin/encodercraft"
    docs="$work/squashfs-root/usr/share/doc/encodercraft"
    share="$work/squashfs-root/usr/share"
    ;;
  *)
    echo "unsupported Linux package: $kind" >&2
    exit 2
    ;;
esac

test "$("$cli" --version)" = "encodercraft-cli $version"
test "$("$gui" --version)" = "encodercraft $version"
for license in LICENSE-MIT LICENSE-APACHE NOTICE; do
  test -s "$docs/$license"
done

"$cli" encode "$fixture" -p "Still Image PNG (Lossless)" -o "$work/out.png" >/dev/null
test -s "$work/out.png"
"$cli" encode "$fixture" -o "$work/out.mp4" >/dev/null
test "$(wc -c < "$work/out.mp4")" -gt 1000

test -s "$share/applications/ai.storyteller.encodercraft.desktop"
test -s "$share/metainfo/ai.storyteller.encodercraft.metainfo.xml"
test -s "$share/icons/hicolor/256x256/apps/ai.storyteller.encodercraft.png"

echo "OK: installed $kind package ($version)"
