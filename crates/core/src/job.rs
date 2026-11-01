//! Job specifications, output targets, and queue status data structures.
//!
//! Matches the inter-app dispatch protocol specification (docs/inter-app-dispatch-protocol.md).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::EncoderError;
use crate::timecode::Rational64;

/// Execution priority of a queued job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum JobPriority {
    Low,
    #[default]
    Normal,
    High,
    Urgent,
}

/// Execution start policy upon submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StartPolicy {
    /// Begins encoding as soon as a worker becomes available.
    #[default]
    AutoStart,
    /// Adds to queue in a paused state until explicitly started by the user.
    Paused,
}

/// Current status of a job or output target in the queue.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum JobStatus {
    #[default]
    Queued,
    Processing,
    Paused,
    Completed,
    Failed(String),
    Cancelled,
}

/// Identifying metadata about the submitting client application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub app_name: String,
    pub app_version: String,
    pub pid: u32,
    #[serde(default)]
    pub user_notes: Option<String>,
}

/// Single clip descriptor inside a multi-clip stitched sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StitchedClipSource {
    pub path: PathBuf,
    #[serde(default)]
    pub in_point_ticks: Option<u64>,
    #[serde(default)]
    pub out_point_ticks: Option<u64>,
}

/// Caption export format for sidecar generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptionExportFormat {
    Srt,
    WebVtt,
    Scc,
    Mcc,
    Ttml,
}

impl CaptionExportFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Srt => "srt",
            Self::WebVtt => "vtt",
            Self::Scc => "scc",
            Self::Mcc => "mcc",
            Self::Ttml => "ttml",
        }
    }
}

/// Mode of media source ingestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SourceDescriptor {
    /// File source already present on disk.
    File {
        path: PathBuf,
        #[serde(default)]
        start_tick: Option<i64>,
        #[serde(default)]
        duration_ticks: Option<i64>,
        #[serde(default)]
        timebase: Option<Rational64>,
    },
    /// Multiple sequential clips stitched together into a single continuous sequence.
    StitchedClips { clips: Vec<StitchedClipSource> },
    /// Live frame streaming over local IPC domain socket or named pipe.
    Pipe { endpoint: String, width: u32, height: u32, frame_rate: Rational64, sample_rate: u32, channels: u32 },
    /// Headless project delegation (renders via origin app's CLI).
    ProjectReference { project_path: PathBuf, composition_id: String, host_cli_path: Option<PathBuf> },
}

/// High-level output media container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContainerType {
    IsoBmffMp4,
    QuickTimeMov,
    MatroskaMkv,
    WebM,
    MxfOp1a,
    MxfOpAtom,
    Wave,
    Aiff,
    Flac,
    StillSequence,
    AnimatedGif,
    Gltf,
    Usdz,
    Pdf,
}

/// Output target specifications.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputTarget {
    pub target_id: String,
    pub output_path: PathBuf,
    pub preset_name: String,
    pub container: ContainerType,
    #[serde(default)]
    pub custom_width: Option<u32>,
    #[serde(default)]
    pub custom_height: Option<u32>,
    #[serde(default)]
    pub custom_frame_rate: Option<Rational64>,
    #[serde(default)]
    pub target_bitrate_kbps: Option<u32>,
    #[serde(default)]
    pub target_lufs: Option<f32>,
    #[serde(default)]
    pub lut_path: Option<PathBuf>,
    #[serde(default)]
    pub burn_timecode: bool,
    #[serde(default)]
    pub caption_path: Option<PathBuf>,
    #[serde(default)]
    pub caption_format: Option<CaptionExportFormat>,
    #[serde(default)]
    pub burn_captions: bool,
    #[serde(default)]
    pub caption_offset_ms: Option<i64>,
}

impl Default for OutputTarget {
    fn default() -> Self {
        Self {
            target_id: String::new(),
            output_path: PathBuf::new(),
            preset_name: String::new(),
            container: ContainerType::IsoBmffMp4,
            custom_width: None,
            custom_height: None,
            custom_frame_rate: None,
            target_bitrate_kbps: None,
            target_lufs: None,
            lut_path: None,
            burn_timecode: false,
            caption_path: None,
            caption_format: None,
            burn_captions: false,
            caption_offset_ms: None,
        }
    }
}

/// Complete, self-contained transcode job specification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CraftJobSpec {
    pub version: String,
    pub job_id: String,
    pub client_info: ClientInfo,
    #[serde(default)]
    pub priority: JobPriority,
    #[serde(default)]
    pub start_policy: StartPolicy,
    pub source: SourceDescriptor,
    pub targets: Vec<OutputTarget>,
}

impl CraftJobSpec {
    /// Validates the internal consistency of the job specification.
    pub fn validate(&self) -> Result<(), EncoderError> {
        if self.job_id.trim().is_empty() {
            return Err(EncoderError::InvalidJobSpec("jobId cannot be empty".into()));
        }

        if self.targets.is_empty() {
            return Err(EncoderError::InvalidJobSpec("job must define at least one output target".into()));
        }

        for (idx, target) in self.targets.iter().enumerate() {
            if target.target_id.trim().is_empty() {
                return Err(EncoderError::InvalidJobSpec(format!("target at index {idx} has empty targetId")));
            }
            if target.output_path.as_os_str().is_empty() {
                return Err(EncoderError::InvalidJobSpec(format!("target `{}` has empty outputPath", target.target_id)));
            }
        }

        Ok(())
    }

    /// Serializes this job specification to a JSON string.
    pub fn to_json(&self) -> Result<String, EncoderError> {
        serde_json::to_string_pretty(self).map_err(|e| EncoderError::InvalidJobSpec(format!("JSON serialization failed: {e}")))
    }

    /// Deserializes a job specification from a JSON string.
    pub fn from_json(json_str: &str) -> Result<Self, EncoderError> {
        let spec: Self = serde_json::from_str(json_str).map_err(|e| EncoderError::InvalidJobSpec(format!("JSON deserialization failed: {e}")))?;
        spec.validate()?;
        Ok(spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_spec_json_roundtrip() {
        let spec = CraftJobSpec {
            version: "1.0.0".into(),
            job_id: "test_job_001".into(),
            client_info: ClientInfo { app_name: "FilmCraft".into(), app_version: "0.9.4".into(), pid: 1234, user_notes: Some("Test export".into()) },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: PathBuf::from("/media/test.mov"), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![OutputTarget {
                target_id: "tgt_01".into(),
                output_path: PathBuf::from("/media/out.mp4"),
                preset_name: "YouTube 4K".into(),
                container: ContainerType::IsoBmffMp4,
                custom_width: Some(3840),
                custom_height: Some(2160),
                custom_frame_rate: Some(Rational64::FPS_60),
                target_bitrate_kbps: Some(45000),
                target_lufs: Some(-14.0),
                lut_path: None,
                burn_timecode: false,
                ..Default::default()
            }],
        };

        let json = spec.to_json().unwrap();
        let deserialized = CraftJobSpec::from_json(&json).unwrap();
        assert_eq!(spec, deserialized);
    }
}
