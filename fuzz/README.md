# EncoderCraft fuzz targets

These `cargo-fuzz` targets exercise container probing, video/audio/still-image decoding, preset
JSON, queue-journal recovery, and safe IPC dispatch. Inputs are capped in each target before they
reach a parser or decoder. The nightly workflow seeds the targets from the checked-in media fixtures,
the BMP smoke fixture, and small valid WAV/JSON examples, then time-boxes each run.

Run one target locally with nightly Rust and `cargo-fuzz` installed:

```sh
cargo install cargo-fuzz
cargo +nightly fuzz run demux_probe
```

Available targets: `demux_probe`, `media_decode`, `image_decode`, `audio_decode`, `preset_json`,
`queue_journal`, and `ipc_message`.
