//! Common muxer traits and sample descriptors.

use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{ChannelLayout, Rational64};
use encodercraft_geom::Dimensions;

/// Parameters for initializing a video output stream in a container.
#[derive(Debug, Clone)]
pub struct VideoStreamConfig {
    pub codec_fourcc: [u8; 4],
    pub dimensions: Dimensions,
    pub frame_rate: Rational64,
    pub pixel_format: PixelFormat,
    pub color_space: ColorSpaceTag,
    pub bit_depth: u8,
    pub codec_private: Vec<u8>,
}

/// Parameters for initializing an audio output stream in a container.
#[derive(Debug, Clone)]
pub struct AudioStreamConfig {
    pub codec_fourcc: [u8; 4],
    pub sample_rate: u32,
    pub layout: ChannelLayout,
    pub bits_per_sample: u16,
    pub codec_private: Vec<u8>,
}

/// An elementary encoded video packet ready for multiplexing.
#[derive(Debug, Clone)]
pub struct VideoSample<'a> {
    pub data: &'a [u8],
    pub pts: i64,
    pub dts: i64,
    pub duration_ticks: u32,
    pub is_keyframe: bool,
}

/// An elementary encoded audio packet ready for multiplexing.
#[derive(Debug, Clone)]
pub struct AudioSample<'a> {
    pub data: &'a [u8],
    pub pts: i64,
    pub duration_ticks: u32,
}

/// Broadcast Wave Format (BWF) metadata for embedding in `bext` chunk.
#[derive(Debug, Clone)]
pub struct BextMetadata {
    pub description: String,
    pub originator: String,
    pub originator_reference: String,
    pub origination_date: String,
    pub origination_time: String,
    pub time_reference_samples: u64,
    pub umid: [u8; 64],
}

impl Default for BextMetadata {
    fn default() -> Self {
        Self {
            description: String::new(),
            originator: String::new(),
            originator_reference: String::new(),
            origination_date: String::new(),
            origination_time: String::new(),
            time_reference_samples: 0,
            umid: [0u8; 64],
        }
    }
}
