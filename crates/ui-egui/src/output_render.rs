//! On-demand output preview: renders a short window of the selected job through the real
//! transcode pipeline (same scaling, colour and encoder as the final export) into a temporary
//! file that the preview monitor then decodes like any other media.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use encodercraft_core::{CraftJobSpec, EncoderError, Rational64, SourceDescriptor};
use encodercraft_pipeline::TranscodePipeline;

/// Longest window rendered for one preview, in seconds of source time.
pub const WINDOW_SECONDS: u64 = 2;

pub struct OutputRender {
    pub base_frame: u64,
    pub path: PathBuf,
    result: Receiver<Result<(), String>>,
}

impl std::fmt::Debug for OutputRender {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutputRender").field("base_frame", &self.base_frame).field("path", &self.path).finish()
    }
}

/// Builds the spec for a preview render: first target only, trimmed to a window starting at
/// `frame`, written below `dir`. Returns the spec and the path it will write.
pub fn preview_spec(job: &CraftJobSpec, frame: u64, fps: f64, frame_count: u64, dir: &Path) -> Result<(CraftJobSpec, PathBuf), EncoderError> {
    let mut spec = job.clone();
    let target = spec.targets.first().cloned().ok_or_else(|| EncoderError::InvalidJobSpec("job has no output target".into()))?;
    spec.targets = vec![target.clone()];
    let name = target.output_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "preview".into());
    let out = dir.join(format!("{}-f{frame}-{name}", sanitize(&job.job_id)));
    spec.targets[0].output_path = out.clone();
    spec.targets[0].caption_path = None;
    spec.targets[0].metadata = None;

    if frame_count > 1
        && let SourceDescriptor::File { start_tick, duration_ticks, timebase, .. } = &mut spec.source
    {
        let rate = fps.round().max(1.0) as i64;
        *start_tick = Some(frame as i64);
        let window = (rate as u64).saturating_mul(WINDOW_SECONDS).min(frame_count.saturating_sub(frame)).max(1);
        *duration_ticks = Some(window as i64);
        *timebase = Some(Rational64::new(1, rate)?);
    }
    Ok((spec, out))
}

fn sanitize(id: &str) -> String {
    id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect()
}

impl OutputRender {
    /// Starts rendering on a background thread.
    pub fn start(job: &CraftJobSpec, frame: u64, fps: f64, frame_count: u64) -> Result<Self, EncoderError> {
        let dir = std::env::temp_dir().join(format!("encodercraft-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        let (spec, path) = preview_spec(job, frame, fps, frame_count, &dir)?;
        let (tx, rx) = channel();
        let worker_path = path.clone();
        std::thread::Builder::new()
            .name("encodercraft-preview-render".into())
            .spawn(move || {
                let outcome = TranscodePipeline::new(spec).execute().map_err(|e| e.to_string());
                if outcome.is_err() {
                    let _ = std::fs::remove_file(&worker_path);
                }
                let _ = tx.send(outcome);
            })
            .map_err(|e| EncoderError::Internal(format!("could not start the preview render thread: {e}")))?;
        Ok(Self { base_frame: frame, path, result: rx })
    }

    /// `Some` once the render finished: `Ok` when the file is ready.
    pub fn poll(&self) -> Option<Result<(), String>> {
        self.result.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_core::{ClientInfo, ContainerType, JobPriority, OutputTarget, StartPolicy};

    fn job(source: &Path) -> CraftJobSpec {
        CraftJobSpec {
            version: "1.0".into(),
            job_id: "a/b c".into(),
            client_info: ClientInfo { app_name: "t".into(), app_version: "0".into(), pid: 1, user_notes: None },
            priority: JobPriority::Normal,
            start_policy: StartPolicy::AutoStart,
            source: SourceDescriptor::File { path: source.to_path_buf(), start_tick: None, duration_ticks: None, timebase: None },
            targets: vec![
                OutputTarget { output_path: "/final/out.mp4".into(), container: ContainerType::IsoBmffMp4, ..Default::default() },
                OutputTarget { output_path: "/final/other.mov".into(), ..Default::default() },
            ],
        }
    }

    #[test]
    fn preview_spec_trims_to_a_window_and_redirects_output() {
        let dir = Path::new("/tmp/previews");
        let (spec, out) = preview_spec(&job(Path::new("in.mov")), 100, 25.0, 1000, dir).unwrap();
        assert_eq!(spec.targets.len(), 1);
        assert_eq!(spec.targets[0].output_path, out);
        assert_eq!(out, dir.join("a_b_c-f100-out.mp4"));
        let SourceDescriptor::File { start_tick, duration_ticks, timebase, .. } = spec.source else { return };
        assert_eq!((start_tick, duration_ticks), (Some(100), Some(50)));
        assert_eq!(timebase, Some(Rational64::new(1, 25).unwrap()));
    }

    #[test]
    fn window_never_runs_past_the_end_and_stills_are_not_trimmed() {
        let (spec, _) = preview_spec(&job(Path::new("in.mov")), 990, 25.0, 1000, Path::new("/tmp")).unwrap();
        let SourceDescriptor::File { duration_ticks, .. } = spec.source else { return };
        assert_eq!(duration_ticks, Some(10));
        let (still, _) = preview_spec(&job(Path::new("a.png")), 0, 30.0, 1, Path::new("/tmp")).unwrap();
        let SourceDescriptor::File { start_tick, .. } = still.source else { return };
        assert_eq!(start_tick, None);
    }

    #[test]
    fn job_without_targets_is_an_error() {
        let mut j = job(Path::new("in.mov"));
        j.targets.clear();
        assert!(preview_spec(&j, 0, 25.0, 100, Path::new("/tmp")).is_err());
    }

    #[test]
    fn rendering_a_still_through_the_pipeline_produces_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in.bmp");
        let frame = encodercraft_core::VideoFrame::new_allocated(
            encodercraft_geom::Dimensions::new(8, 8).unwrap(),
            encodercraft_color::PixelFormat::Rgba8,
            encodercraft_color::ColorSpaceTag::SRGB,
            0,
            1,
            Rational64 { num: 1, den: 1 },
        )
        .unwrap();
        std::fs::write(&src, encodercraft_image_codecs::encode_bmp(&frame).unwrap()).unwrap();
        let mut j = job(&src);
        j.targets[0].container = ContainerType::Png;
        j.targets[0].output_path = "final.png".into();
        let render = OutputRender::start(&j, 0, 30.0, 1).unwrap();
        let outcome = loop {
            if let Some(r) = render.poll() {
                break r;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(outcome, Ok(()));
        assert!(render.path.metadata().unwrap().len() > 0);
        let _ = std::fs::remove_file(&render.path);
    }
}
