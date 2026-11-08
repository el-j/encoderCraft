use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{AudioSampleBuffer, ChannelLayout, Rational64};
use encodercraft_geom::Dimensions;
use encodercraft_muxers::{BextMetadata, BwfWavWriter, MkvMuxerBuilder, Mp4Muxer, VideoSample, VideoStreamConfig, WavFormatTag};
use proptest::prelude::*;
use std::io::Cursor;

proptest! {
    #[test]
    fn test_bwf_wav_no_panic(
        sample_rate in 8000u32..96000u32,
        channels in 1u16..8u16,
        sample_count in 0usize..64,
    ) {
        let cursor = Cursor::new(Vec::new());
        let bext = BextMetadata {
            description: "Fuzz Test".into(),
            originator: "encoderCraft".into(),
            ..Default::default()
        };

        if let Ok(mut writer) = BwfWavWriter::new(cursor, sample_rate, channels, WavFormatTag::Pcm16, Some(&bext)) {
            let layout = match channels {
                1 => ChannelLayout::Mono,
                2 => ChannelLayout::Stereo,
                6 => ChannelLayout::Surround51,
                _ => ChannelLayout::Stereo,
            };

            let mut chs = Vec::new();
            for _ in 0..(layout as usize) {
                chs.push(vec![0.1f32; sample_count]);
            }

            if let Ok(buf) = AudioSampleBuffer::from_planar_channels(sample_rate, layout, 0, chs) {
                let _ = writer.write_buffer(&buf);
                let _ = writer.finish();
            }
        }
    }

    #[test]
    fn test_mp4_muxer_no_panic(
        w in 16u32..256u32,
        h in 16u32..256u32,
        frame_bytes in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let cursor = Cursor::new(Vec::new());
        if let Ok(mut muxer) = Mp4Muxer::new(cursor, false)
            && let Ok(dims) = Dimensions::new(w, h)
        {
            let cfg = VideoStreamConfig {
                codec_fourcc: *b"avc1",
                dimensions: dims,
                frame_rate: Rational64::FPS_24,
                pixel_format: PixelFormat::Yuv420p,
                color_space: ColorSpaceTag::REC709,
                bit_depth: 8,
                codec_private: vec![0, 0, 0, 1],
                spatial: None,
            };

            if let Ok(track) = muxer.add_video_stream(&cfg) {
                let _ = muxer.write_video_sample(track, VideoSample {
                    data: &frame_bytes,
                    pts: 0,
                    dts: 0,
                    duration_ticks: 1000,
                    is_keyframe: true,
                });
                let _: Result<Cursor<Vec<u8>>, _> = muxer.finish(false);
            }
        }
    }

    #[test]
    fn test_mkv_muxer_no_panic(
        w in 16u32..256u32,
        h in 16u32..256u32,
        frame_bytes in prop::collection::vec(any::<u8>(), 1..64),
    ) {
        let cursor = Cursor::new(Vec::new());
        let mut builder = MkvMuxerBuilder::new(cursor, false);
        if let Ok(dims) = Dimensions::new(w, h) {
            let cfg = VideoStreamConfig {
                codec_fourcc: *b"av01",
                dimensions: dims,
                frame_rate: Rational64::FPS_24,
                pixel_format: PixelFormat::Yuv420p,
                color_space: ColorSpaceTag::REC709,
                bit_depth: 8,
                codec_private: vec![],
                spatial: None,
            };
            let track = builder.add_video_stream(&cfg);
            if let Ok(mut muxer) = builder.build() {
                let _ = muxer.write_video_sample(track, VideoSample {
                    data: &frame_bytes,
                    pts: 0,
                    dts: 0,
                    duration_ticks: 40,
                    is_keyframe: true,
                });
                let _ = muxer.finish();
            }
        }
    }
}
