#!/usr/bin/env python3
"""Generate deterministic malformed-media seeds for the bounded nightly fuzz run."""

import argparse
import struct
from pathlib import Path


def write_seed(root: Path, target: str, name: str, data: bytes) -> None:
    directory = root / target
    directory.mkdir(parents=True, exist_ok=True)
    (directory / name).write_bytes(data)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus_root", type=Path)
    root = parser.parse_args().corpus_root
    fixtures = Path(__file__).resolve().parents[1] / "crates/source/tests/fixtures"
    h264 = (fixtures / "h264.mp4").read_bytes()
    truncated = h264[: max(1, len(h264) // 2)]

    wave = (
        b"RIFF"
        + struct.pack("<I", 0xFFFFFFFF)
        + b"WAVEfmt "
        + struct.pack("<IHHIIHH", 16, 1, 1, 48_000, 96_000, 2, 16)
        + b"data"
        + struct.pack("<I", 0xFFFFFFFF)
    )
    png = (
        b"\x89PNG\r\n\x1a\n"
        + struct.pack(">I", 13)
        + b"IHDR"
        + struct.pack(">II", 0xFFFFFFFF, 0xFFFFFFFF)
        + bytes((16, 6, 0, 0, 0, 0, 0, 0, 0))
    )
    write_seed(root, "demux_probe", "truncated-h264.mp4", truncated)
    write_seed(root, "media_decode", "truncated-h264.mp4", truncated)
    write_seed(root, "audio_decode", "oversized-wave.wav", wave)
    write_seed(root, "image_decode", "oversized-png.png", png)
    write_seed(root, "preset_json", "oversized-dimensions.json", b'{"width":4294967295,"height":4294967295}')
    write_seed(root, "queue_journal", "truncated-journal.json", b'{"version":1,"jobs":[')
    write_seed(root, "ipc_message", "invalid-json.json", b'{"id":1,"method":"queue.submit","params":[')


if __name__ == "__main__":
    main()
