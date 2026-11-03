# EncoderCraft app icon (placeholder)

A queue of three shrinking bars feeding a play triangle: media waiting to be transcoded.

This is a **placeholder** until the owner draws EncoderCraft's icon in ArtCraft like the other apps'
icons. Replace `icon.svg` and run `packaging/icons.sh`; nothing else needs to change.

## Palette

| Colour | Hex | Used for |
|---|---|---|
| Ink | `#0b0b0c` | Triangle outline |
| Paper | `#efe9dc` | Bars and triangle |
| EncoderCraft violet (app colour) | `#5b3df5` | The full-bleed background field |

## Geometry

A 512 × 512 viewBox, clipped to a rounded square with `rx=112`. Windows and Linux use the full
tile. The macOS icons (`encodercraft.icns`, `encodercraft-macos-512.png`) sit on Apple's grid: an
824/1024 body with a transparent margin.

## Provenance and licence

Drawn from scratch as hand-placed SVG shapes by the EncoderCraft contributors; no source image was
traced or copied, and nothing is derived from Adobe artwork. MIT OR Apache-2.0, like the code.

## Files

| File | What it is |
|---|---|
| `icon.svg` | The canonical vector |
| `encodercraft-1024.png` | 1024 px render |
| `encodercraft-macos-512.png` | Runtime window/Dock icon on macOS (Apple grid margin) |
| `encodercraft.icns` | macOS bundle icon |
| `encodercraft.ico` | Windows icon (16–256 px), embedded in `encodercraft.exe` and used by the MSI |
| `hicolor/<size>x<size>/apps/ai.storyteller.encodercraft.png` | Linux/FreeBSD icon theme, 16–512 px, plus `scalable/` |

Regenerate everything with `packaging/icons.sh` (needs `resvg`, and `iconutil` on macOS for the
`.icns`).
