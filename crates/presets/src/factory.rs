//! Built-in factory presets for web, broadcast, social, and audio workflows.

use encodercraft_color::PixelFormat;
use encodercraft_core::{ChannelLayout, ContainerType, Rational64};
use encodercraft_geom::ScaleMode;

use crate::model::{EncodingPreset, MatchSourceOr, PresetAudioSettings, PresetCategory, PresetVideoSettings};

/// Returns the standard library of embedded factory presets.
pub fn factory_presets() -> Vec<EncodingPreset> {
    vec![
        // 1. Match Source - Adaptive High Bitrate
        EncodingPreset {
            name: "Match Source - High Bitrate".into(),
            description: "Matches source dimensions, frame rate, and channels with high-quality H.264 video and AAC audio".into(),
            category: PresetCategory::Web,
            container: ContainerType::IsoBmffMp4,
            video: Some(PresetVideoSettings {
                codec: "h264".into(),
                width: MatchSourceOr::MatchSource,
                height: MatchSourceOr::MatchSource,
                frame_rate: MatchSourceOr::MatchSource,
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv420p),
                target_bitrate_kbps: 12000,
                keyframe_interval: 48,
            }),
            audio: Some(PresetAudioSettings {
                codec: "aac".into(),
                sample_rate: MatchSourceOr::MatchSource,
                layout: MatchSourceOr::MatchSource,
                bitrate_kbps: Some(320),
                target_lufs: Some(-14.0),
            }),
        },
        // 2. YouTube 4K Ultra HD
        EncodingPreset {
            name: "YouTube 4K Ultra HD".into(),
            description: "3840x2160 UHD at up to 60 fps optimized for YouTube HDR/SDR upload".into(),
            category: PresetCategory::Web,
            container: ContainerType::IsoBmffMp4,
            video: Some(PresetVideoSettings {
                codec: "h264".into(),
                width: MatchSourceOr::Exact(3840),
                height: MatchSourceOr::Exact(2160),
                frame_rate: MatchSourceOr::MatchSource,
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv420p),
                target_bitrate_kbps: 45000,
                keyframe_interval: 60,
            }),
            audio: Some(PresetAudioSettings {
                codec: "aac".into(),
                sample_rate: MatchSourceOr::Exact(48000),
                layout: MatchSourceOr::Exact(ChannelLayout::Stereo),
                bitrate_kbps: Some(384),
                target_lufs: Some(-14.0),
            }),
        },
        // 3. YouTube 1080p Full HD
        EncodingPreset {
            name: "YouTube 1080p Full HD".into(),
            description: "1920x1080 standard web delivery for YouTube, Vimeo, and Facebook".into(),
            category: PresetCategory::Web,
            container: ContainerType::IsoBmffMp4,
            video: Some(PresetVideoSettings {
                codec: "h264".into(),
                width: MatchSourceOr::Exact(1920),
                height: MatchSourceOr::Exact(1080),
                frame_rate: MatchSourceOr::MatchSource,
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv420p),
                target_bitrate_kbps: 12000,
                keyframe_interval: 48,
            }),
            audio: Some(PresetAudioSettings {
                codec: "aac".into(),
                sample_rate: MatchSourceOr::Exact(48000),
                layout: MatchSourceOr::Exact(ChannelLayout::Stereo),
                bitrate_kbps: Some(320),
                target_lufs: Some(-14.0),
            }),
        },
        // 4. Apple ProRes 422 HQ
        EncodingPreset {
            name: "Apple ProRes 422 HQ".into(),
            description: "Broadcast-mastering Apple ProRes 422 HQ with uncompressed Linear PCM audio in QuickTime MOV".into(),
            category: PresetCategory::Broadcast,
            container: ContainerType::QuickTimeMov,
            video: Some(PresetVideoSettings {
                codec: "prores".into(),
                width: MatchSourceOr::MatchSource,
                height: MatchSourceOr::MatchSource,
                frame_rate: MatchSourceOr::MatchSource,
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv422p10),
                target_bitrate_kbps: 220000,
                keyframe_interval: 1, // Intra-frame
            }),
            audio: Some(PresetAudioSettings {
                codec: "pcm_s24le".into(),
                sample_rate: MatchSourceOr::Exact(48000),
                layout: MatchSourceOr::MatchSource,
                bitrate_kbps: None,
                target_lufs: None,
            }),
        },
        // 4b. Avid DNxHR HQ 1080p
        EncodingPreset {
            name: "Avid DNxHR HQ 1080p".into(),
            description: "Avid DNxHR HQ 8-bit 4:2:2 mastering codec for broadcast television and post-production".into(),
            category: PresetCategory::Broadcast,
            container: ContainerType::QuickTimeMov,
            video: Some(PresetVideoSettings {
                codec: "dnxhr".into(),
                width: MatchSourceOr::Exact(1920),
                height: MatchSourceOr::Exact(1080),
                frame_rate: MatchSourceOr::MatchSource,
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv422p),
                target_bitrate_kbps: 220000,
                keyframe_interval: 1, // Intra-frame
            }),
            audio: Some(PresetAudioSettings {
                codec: "pcm_s24le".into(),
                sample_rate: MatchSourceOr::Exact(48000),
                layout: MatchSourceOr::MatchSource,
                bitrate_kbps: None,
                target_lufs: None,
            }),
        },
        // 4c. Avid DNxHR HQX 4K (10-bit)
        EncodingPreset {
            name: "Avid DNxHR HQX 4K (10-bit)".into(),
            description: "Avid DNxHR HQX 10-bit 4:2:2 UHD broadcast master with high dynamic range headroom".into(),
            category: PresetCategory::Broadcast,
            container: ContainerType::QuickTimeMov,
            video: Some(PresetVideoSettings {
                codec: "dnxhr".into(),
                width: MatchSourceOr::Exact(3840),
                height: MatchSourceOr::Exact(2160),
                frame_rate: MatchSourceOr::MatchSource,
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv422p10),
                target_bitrate_kbps: 880000,
                keyframe_interval: 1, // Intra-frame
            }),
            audio: Some(PresetAudioSettings {
                codec: "pcm_s24le".into(),
                sample_rate: MatchSourceOr::Exact(48000),
                layout: MatchSourceOr::MatchSource,
                bitrate_kbps: None,
                target_lufs: None,
            }),
        },
        // 4d. Broadcast MXF OP1a DNxHR HQ
        EncodingPreset {
            name: "Broadcast MXF OP1a DNxHR HQ".into(),
            description: "SMPTE ST 377-1 / ST 378 MXF OP1a broadcast master with Avid DNxHR HQ VC-3 video and 24-bit 48 kHz PCM audio".into(),
            category: PresetCategory::Broadcast,
            container: ContainerType::MxfOp1a,
            video: Some(PresetVideoSettings {
                codec: "dnxhr".into(),
                width: MatchSourceOr::Exact(1920),
                height: MatchSourceOr::Exact(1080),
                frame_rate: MatchSourceOr::MatchSource,
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv422p),
                target_bitrate_kbps: 220000,
                keyframe_interval: 1,
            }),
            audio: Some(PresetAudioSettings {
                codec: "pcm_s24le".into(),
                sample_rate: MatchSourceOr::Exact(48000),
                layout: MatchSourceOr::MatchSource,
                bitrate_kbps: None,
                target_lufs: Some(-23.0),
            }),
        },
        // 4e. Avid Media Composer MXF OP-Atom
        EncodingPreset {
            name: "Avid Media Composer MXF OP-Atom".into(),
            description: "SMPTE ST 390 MXF OP-Atom clip-wrapped Avid DNxHR HQ essence for direct Avid MediaFiles ingestion".into(),
            category: PresetCategory::Broadcast,
            container: ContainerType::MxfOpAtom,
            video: Some(PresetVideoSettings {
                codec: "dnxhr".into(),
                width: MatchSourceOr::Exact(1920),
                height: MatchSourceOr::Exact(1080),
                frame_rate: MatchSourceOr::MatchSource,
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv422p),
                target_bitrate_kbps: 220000,
                keyframe_interval: 1,
            }),
            audio: None,
        },
        // 5. TikTok / Instagram Reels 9:16 Vertical
        EncodingPreset {
            name: "TikTok / Reels 9:16 Vertical".into(),
            description: "1080x1920 vertical portrait video optimized for mobile platforms".into(),
            category: PresetCategory::SocialMedia,
            container: ContainerType::IsoBmffMp4,
            video: Some(PresetVideoSettings {
                codec: "h264".into(),
                width: MatchSourceOr::Exact(1080),
                height: MatchSourceOr::Exact(1920),
                frame_rate: MatchSourceOr::Exact(Rational64::FPS_30),
                scale_mode: ScaleMode::Fit,
                pixel_format: MatchSourceOr::Exact(PixelFormat::Yuv420p),
                target_bitrate_kbps: 8000,
                keyframe_interval: 30,
            }),
            audio: Some(PresetAudioSettings {
                codec: "aac".into(),
                sample_rate: MatchSourceOr::Exact(44100),
                layout: MatchSourceOr::Exact(ChannelLayout::Stereo),
                bitrate_kbps: Some(256),
                target_lufs: Some(-14.0),
            }),
        },
        // 6. Broadcast Wave BWF 48kHz EBU R128
        EncodingPreset {
            name: "Broadcast Wave BWF (-23 LUFS)".into(),
            description: "EBU R128 / ITU-R BS.1770 broadcast audio delivery at 48 kHz / 24-bit with timecode bext chunk".into(),
            category: PresetCategory::Audio,
            container: ContainerType::Wave,
            video: None,
            audio: Some(PresetAudioSettings {
                codec: "pcm_s24le".into(),
                sample_rate: MatchSourceOr::Exact(48000),
                layout: MatchSourceOr::MatchSource,
                bitrate_kbps: None,
                target_lufs: Some(-23.0),
            }),
        },
        // 7. Podcast Streaming (-16 LUFS)
        EncodingPreset {
            name: "Podcast Master (-16 LUFS)".into(),
            description: "Apple Podcasts / Spotify compliant master at -16 LUFS True-Peak limited".into(),
            category: PresetCategory::Audio,
            container: ContainerType::Wave,
            video: None,
            audio: Some(PresetAudioSettings {
                codec: "pcm_s16le".into(),
                sample_rate: MatchSourceOr::Exact(44100),
                layout: MatchSourceOr::Exact(ChannelLayout::Stereo),
                bitrate_kbps: None,
                target_lufs: Some(-16.0),
            }),
        },
        // 8. 3D glTF 2.0 Web Delivery
        EncodingPreset {
            name: "glTF 2.0 Binary (.glb)".into(),
            description: "Khronos glTF 2.0 binary mesh container for interactive 3D and WebGL".into(),
            category: PresetCategory::Spatial3D,
            container: ContainerType::Gltf,
            video: None,
            audio: None,
        },
        // 9. Lossless FLAC Master (24-bit 48kHz)
        EncodingPreset {
            name: "Lossless FLAC Master (24-bit 48kHz)".into(),
            description: "Lossless Free Lossless Audio Codec (FLAC) 24-bit master at 48 kHz with target loudness normalization".into(),
            category: PresetCategory::Audio,
            container: ContainerType::Flac,
            video: None,
            audio: Some(PresetAudioSettings {
                codec: "flac".into(),
                sample_rate: MatchSourceOr::Exact(48000),
                layout: MatchSourceOr::MatchSource,
                bitrate_kbps: None,
                target_lufs: Some(-16.0),
            }),
        },
        // 10. Apple Lossless AIFF (24-bit 48kHz)
        EncodingPreset {
            name: "Apple Lossless AIFF (24-bit 48kHz)".into(),
            description: "Uncompressed Apple AIFF 24-bit linear PCM audio master at 48 kHz".into(),
            category: PresetCategory::Audio,
            container: ContainerType::Aiff,
            video: None,
            audio: Some(PresetAudioSettings {
                codec: "pcm_s24be".into(),
                sample_rate: MatchSourceOr::Exact(48000),
                layout: MatchSourceOr::MatchSource,
                bitrate_kbps: None,
                target_lufs: None,
            }),
        },
        // 11. Web Vector SVG (Optimized)
        EncodingPreset {
            name: "Web Vector SVG (Optimized)".into(),
            description: "W3C Scalable Vector Graphics (SVG) container optimized for web and responsive UI layouts".into(),
            category: PresetCategory::Vector,
            container: ContainerType::Svg,
            video: None,
            audio: None,
        },
        // 12. Print-Ready PDF/X-4
        EncodingPreset {
            name: "Print-Ready PDF/X-4".into(),
            description: "ISO 15930-7 (PDF/X-4) document interchange standard for high-fidelity graphic arts and prepress".into(),
            category: PresetCategory::Document,
            container: ContainerType::Pdf,
            video: None,
            audio: None,
        },
        // 13. Wavefront OBJ (.obj)
        EncodingPreset {
            name: "Wavefront OBJ (.obj)".into(),
            description: "Standard Wavefront OBJ ASCII geometry interchange mesh".into(),
            category: PresetCategory::Spatial3D,
            container: ContainerType::Obj,
            video: None,
            audio: None,
        },
        // 14. 3D Print Binary STL (.stl)
        EncodingPreset {
            name: "3D Print Binary STL (.stl)".into(),
            description: "High-density stereolithography triangle mesh for rapid prototyping and 3D printing".into(),
            category: PresetCategory::Spatial3D,
            container: ContainerType::Stl,
            video: None,
            audio: None,
        },
        // 15. Stanford Polygon PLY (.ply)
        EncodingPreset {
            name: "Stanford Polygon PLY (.ply)".into(),
            description: "Stanford polygon mesh binary format for 3D scanning, photogrammetry, and Gaussian splats".into(),
            category: PresetCategory::Spatial3D,
            container: ContainerType::Ply,
            video: None,
            audio: None,
        },
        // 16. Apple AR Quick Look USDZ (.usdz)
        EncodingPreset {
            name: "Apple AR Quick Look USDZ (.usdz)".into(),
            description: "Pixar Universal Scene Description zip package optimized for Apple iOS/macOS AR Quick Look".into(),
            category: PresetCategory::Spatial3D,
            container: ContainerType::Usdz,
            video: None,
            audio: None,
        },
        // 17. Still Image PNG (Lossless)
        EncodingPreset {
            name: "Still Image PNG (Lossless)".into(),
            description: "W3C Portable Network Graphics 8-bit RGBA lossless raster image with alpha channel".into(),
            category: PresetCategory::StillImage,
            container: ContainerType::Png,
            video: None,
            audio: None,
        },
        // 18. Broadcast TIFF (16-bit RGB)
        EncodingPreset {
            name: "Broadcast TIFF (16-bit RGB)".into(),
            description: "Baseline TIFF 6.0 uncompressed raster image for color grading and print workflows".into(),
            category: PresetCategory::StillImage,
            container: ContainerType::Tiff,
            video: None,
            audio: None,
        },
        // 19. VFX OpenEXR (Half Float)
        EncodingPreset {
            name: "VFX OpenEXR (Half Float)".into(),
            description: "Industrial Light & Magic OpenEXR 16-bit half-float high dynamic range raster master".into(),
            category: PresetCategory::StillImage,
            container: ContainerType::OpenExr,
            video: None,
            audio: None,
        },
        // 20. Windows Bitmap BMP (Uncompressed)
        EncodingPreset {
            name: "Windows Bitmap BMP (Uncompressed)".into(),
            description: "Universal uncompressed Windows Bitmap DIB raster format".into(),
            category: PresetCategory::StillImage,
            container: ContainerType::Bmp,
            video: None,
            audio: None,
        },
        // 21. Still Image JPEG
        EncodingPreset {
            name: "Still Image JPEG".into(),
            description: "Baseline JPEG 8-bit still image; transparency is flattened onto white".into(),
            category: PresetCategory::StillImage,
            container: ContainerType::Jpeg,
            video: None,
            audio: None,
        },
        // 22. Still Image WebP (Lossless)
        EncodingPreset {
            name: "Still Image WebP (Lossless)".into(),
            description: "Lossless WebP still image with alpha channel".into(),
            category: PresetCategory::StillImage,
            container: ContainerType::WebP,
            video: None,
            audio: None,
        },
    ]
}
