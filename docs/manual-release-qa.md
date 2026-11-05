# Manual Release QA

This checklist records human checks that automated media and installer tests cannot establish.
Complete it against the exact release candidate before publication and retain the evidence with
the release record. Do not mark a check complete based only on generated test patterns.

The automated `qa_matrix` creates synthetic sources with FFmpeg. It is not evidence that actual
phone footage or screen recordings work. Use non-sensitive, lawfully obtained sample media; do not
commit private source files to the repository.

## Run record

- Candidate version:
- Commit:
- Date:
- Tester:
- OS and version:
- Machine and display/GPU:
- Evidence location:

## Real-source playback and transcode

For each source type, use an actual media file (not an FFmpeg-generated fixture). Run every
applicable preset family through both the CLI and the GUI queue; duplicate rows as needed for each
source/preset pairing. Confirm the outputs open in an independent player and inspect audio/video
synchronization, duration, frame rate, and dimensions. Record exact source and preset identifiers
without including private media.

| Source type | Source provenance / identifier | Preset | CLI | GUI queue | Independent playback / notes |
|---|---|---|---|---|---|
| Phone H.264 MOV | | | [ ] | [ ] | |
| Screen-recording MOV | | | [ ] | [ ] | |
| HEVC MP4 | | | [ ] | [ ] | |
| VP9 WebM | | | [ ] | [ ] | |
| ProRes MOV | | | [ ] | [ ] | |
| MPEG-2 transport stream | | | [ ] | [ ] | |
| WAV | | | [ ] | [ ] | |
| MP3 | | | [ ] | [ ] | |
| FLAC | | | [ ] | [ ] | |
| PNG | | | [ ] | [ ] | |
| JPEG | | | [ ] | [ ] | |
| TIFF | | | [ ] | [ ] | |
| OpenEXR | | | [ ] | [ ] | |
| WebP | | | [ ] | [ ] | |

Also enqueue multiple distinct items in one batch, start the queue once, and verify every item
reaches a terminal state with a valid output.

- [ ] Multi-item batch completed from one Start Queue action.
- [ ] No failed, stuck, or silently skipped items.
- [ ] Audio/video synchronization and output metadata were checked independently.

## Human-only visual checks

- [ ] Compare the running application with the approved reference screenshot at the same window
  size and display scale; record visible differences and attach before/after screenshots.
- [ ] Scrub and play representative motion footage; preview remains responsive without obvious
  stalls, frozen frames, or placeholder imagery.
- [ ] Review output playback for visible corruption, unexpected black frames, and obvious cadence
  problems.
- [ ] Record any issue number for each failure; do not treat an unverified item as passed.

## Sign-off

- Result: PASS / FAIL / BLOCKED
- Open issues:
- Evidence reviewed by:
- Sign-off date:
