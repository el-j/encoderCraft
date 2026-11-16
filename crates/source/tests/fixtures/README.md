# Decoder test fixtures

Tiny (96x64, 15 frames, 25 fps, no audio) streams used by `tests/decoders.rs` and
`tests/fuzz_decoders.rs`. Each was produced by an independent encoder (FFmpeg with
libx264, libx265, libvpx, libvpx-vp9, SVT-AV1, prores_ks and mpeg2video) from the
`testsrc2` generator, so decoding is checked against streams we did not write ourselves.

`reference_luma.raw` holds source frames 0 and 14 as raw 8-bit luma (96x64 each); the tests
compare decoded luma against it by PSNR.

Regenerate with:

    SRC="-f lavfi -i testsrc2=size=96x64:rate=25:duration=0.6"
    N="-an -map_metadata -1 -fflags +bitexact -flags:v +bitexact"
    ffmpeg $SRC $N -c:v mpeg2video -g 6 -bf 2 -b:v 150k -minrate 150k -maxrate 150k -bufsize 100k mpeg2.mpg
    ffmpeg $SRC $N -c:v mpeg2video -g 6 -bf 2 -b:v 150k -f mpegts mpeg2.ts
    ffmpeg $SRC $N -c:v libx265 -pix_fmt yuv420p hevc.mp4
    ffmpeg $SRC $N -c:v libx264 -pix_fmt yuv420p h264.mp4
    ffmpeg $SRC $N -c:v libvpx-vp9 -b:v 200k vp9.webm
    ffmpeg $SRC $N -c:v libvpx -b:v 200k vp8.webm
    ffmpeg $SRC $N -c:v prores_ks -profile:v 0 prores.mov
    ffmpeg $SRC $N -c:v libsvtav1 av1.mp4
    ffmpeg -f lavfi -i "testsrc2=size=96x64:rate=25:duration=0.6" \
      -vf "format=gray,select='eq(n,0)+eq(n,14)'" -fps_mode passthrough -f rawvideo reference_luma.raw

FFmpeg is used only as an external fixture generator; no FFmpeg code is part of this repository.
