//! `stream-encode` and `stream-test`: the receiving and producing ends of a shared-memory pipe.
//!
//! `stream-encode` runs a job whose source is a `Pipe` endpoint, so any application that follows
//! the producer protocol can hand frames over without touching disk. `stream-test` is a producer
//! with a synthetic picture and tone, for trying the handover without another application.

use std::path::PathBuf;
use std::time::Duration;

use encodercraft_core::{ClientInfo, CraftJobSpec, EncoderError, JobPriority, OutputTarget, Rational64, SourceDescriptor, StartPolicy};
use encodercraft_pipeline::TranscodePipeline;
use encodercraft_presets::PresetRegistry;
use encodercraft_source::pipe::{PipeFormat, PipeProducer};

const ATTACH_TIMEOUT: Duration = Duration::from_secs(60);
const DETACH_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_TEST_SECONDS: u32 = 3_600;

struct StreamArgs {
    format: PipeFormat,
    preset: Option<String>,
    output: Option<PathBuf>,
    seconds: u32,
}

fn invalid(message: impl Into<String>) -> EncoderError {
    EncoderError::InvalidJobSpec(message.into())
}

fn parse_size(value: &str) -> Result<(u32, u32), EncoderError> {
    let (w, h) = value.split_once('x').ok_or_else(|| invalid(format!("--size '{value}' must look like 1920x1080")))?;
    let parse = |part: &str| part.parse::<u32>().map_err(|_| invalid(format!("--size '{value}' must look like 1920x1080")));
    Ok((parse(w)?, parse(h)?))
}

fn parse_fps(value: &str) -> Result<Rational64, EncoderError> {
    let bad = || invalid(format!("--fps '{value}' must be a positive number or a fraction such as 30000/1001"));
    let (num, den) = match value.split_once('/') {
        Some((n, d)) => (n.parse::<i64>().map_err(|_| bad())?, d.parse::<i64>().map_err(|_| bad())?),
        None => (value.parse::<i64>().map_err(|_| bad())?, 1),
    };
    if num <= 0 || den <= 0 {
        return Err(bad());
    }
    Ok(Rational64 { num, den })
}

fn parse_audio(value: &str) -> Result<(u32, u32), EncoderError> {
    let bad = || invalid(format!("--audio '{value}' must look like 48000:2 (sample rate:channels)"));
    let (rate, channels) = value.split_once(':').ok_or_else(bad)?;
    Ok((rate.parse().map_err(|_| bad())?, channels.parse().map_err(|_| bad())?))
}

fn parse(args: &[String]) -> Result<StreamArgs, EncoderError> {
    let mut endpoint = None;
    let mut size = None;
    let mut fps = Rational64 { num: 30, den: 1 };
    let mut audio = (0, 0);
    let mut preset = None;
    let mut output = None;
    let mut seconds = 2;

    let mut iter = args.iter();
    while let Some(flag) = iter.next() {
        let mut value = |name: &str| iter.next().cloned().ok_or_else(|| invalid(format!("{name} needs a value")));
        match flag.as_str() {
            "--endpoint" => endpoint = Some(value("--endpoint")?),
            "--size" => size = Some(parse_size(&value("--size")?)?),
            "--fps" => fps = parse_fps(&value("--fps")?)?,
            "--audio" => audio = parse_audio(&value("--audio")?)?,
            "-p" | "--preset" => preset = Some(value("--preset")?),
            "-o" | "--output" => output = Some(PathBuf::from(value("--output")?)),
            "--seconds" => {
                let text = value("--seconds")?;
                seconds = text
                    .parse()
                    .ok()
                    .filter(|s| (1..=MAX_TEST_SECONDS).contains(s))
                    .ok_or_else(|| invalid(format!("--seconds '{text}' must be between 1 and {MAX_TEST_SECONDS}")))?;
            }
            other => return Err(invalid(format!("unknown stream option '{other}'"))),
        }
    }

    let endpoint = endpoint.ok_or_else(|| invalid("--endpoint is required (the shared-memory service name, e.g. encodercraft/demo)"))?;
    let (width, height) = size.unwrap_or((0, 0));
    if width == 0 && audio.1 == 0 {
        return Err(invalid("declare video with --size and/or audio with --audio"));
    }
    Ok(StreamArgs {
        format: PipeFormat { endpoint, width, height, frame_rate: fps, sample_rate: audio.0, channels: audio.1 },
        preset,
        output,
        seconds,
    })
}

/// Encodes whatever a producer streams into the named endpoint.
pub fn cmd_stream_encode(args: &[String], registry: &PresetRegistry) -> Result<(), EncoderError> {
    let parsed = parse(args)?;
    let output = parsed.output.ok_or_else(|| invalid("-o/--output is required"))?;
    let preset_name = parsed.preset.unwrap_or_else(|| "Match Source - High Bitrate".to_string());
    let preset = registry.find_by_name(&preset_name).ok_or_else(|| invalid(format!("preset '{preset_name}' not found")))?;
    let format = parsed.format;

    let spec = CraftJobSpec {
        version: "1.0".into(),
        job_id: format!("stream-{}", std::process::id()),
        client_info: ClientInfo {
            app_name: "encodercraft-cli".into(),
            app_version: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
            user_notes: None,
        },
        priority: JobPriority::Normal,
        start_policy: StartPolicy::AutoStart,
        source: SourceDescriptor::Pipe {
            endpoint: format.endpoint.clone(),
            width: format.width,
            height: format.height,
            frame_rate: format.frame_rate,
            sample_rate: format.sample_rate,
            channels: format.channels,
        },
        targets: vec![OutputTarget {
            target_id: "stream".into(),
            output_path: output.clone(),
            preset_name: preset.name.clone(),
            container: preset.container,
            ..Default::default()
        }],
    };

    println!(
        "Waiting for a producer on '{}' ({}x{} @ {}/{})…",
        format.endpoint, format.width, format.height, format.frame_rate.num, format.frame_rate.den
    );
    TranscodePipeline::new(spec).execute()?;
    println!("Wrote {}", output.display());
    Ok(())
}

/// Streams a moving test pattern (and a 440 Hz tone when audio is declared) to the endpoint.
pub fn cmd_stream_test(args: &[String]) -> Result<(), EncoderError> {
    let parsed = parse(args)?;
    let format = parsed.format;
    let mut producer = PipeProducer::create(format.clone())?;
    println!("Publishing on '{}'; start `stream-encode` with the same options.", format.endpoint);
    if !producer.wait_for_receiver(ATTACH_TIMEOUT) {
        return Err(EncoderError::Io(format!("no receiver attached to '{}' within {} s", format.endpoint, ATTACH_TIMEOUT.as_secs())));
    }

    let fps = format.frame_rate.num as f64 / format.frame_rate.den as f64;
    if format.width > 0 {
        let frames = (f64::from(parsed.seconds) * fps).round() as u64;
        let mut rgba = vec![255u8; (format.width as usize) * (format.height as usize) * 4];
        for i in 0..frames {
            for (n, px) in rgba.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let x = (n as u64) % u64::from(format.width);
                let y = (n as u64) / u64::from(format.width);
                px.copy_from_slice(&[((x * 2 + i * 6) % 256) as u8, ((y * 3 + i * 2) % 256) as u8, 180, 255]);
            }
            producer.send_video(&rgba)?;
        }
    }
    if format.channels > 0 {
        let rate = f64::from(format.sample_rate);
        let channels = format.channels as usize;
        let total = u64::from(format.sample_rate) * u64::from(parsed.seconds);
        let mut chunk = Vec::with_capacity(channels * 4_800);
        let mut index = 0u64;
        while index < total {
            chunk.clear();
            for _ in 0..4_800.min(total - index) {
                let sample = ((index as f64) * 440.0 * std::f64::consts::TAU / rate).sin() as f32 * 0.5;
                chunk.extend(std::iter::repeat_n(sample, channels));
                index += 1;
            }
            producer.send_audio(&chunk)?;
        }
    }
    if !producer.finish(DETACH_TIMEOUT)? {
        return Err(EncoderError::Io("the receiver did not finish reading the stream".into()));
    }
    println!("Stream delivered.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn parses_a_full_declaration() {
        let parsed =
            parse(&args(&["--endpoint", "x/y", "--size", "640x360", "--fps", "30000/1001", "--audio", "48000:2", "--seconds", "5"])).unwrap();
        assert_eq!((parsed.format.width, parsed.format.height), (640, 360));
        assert_eq!((parsed.format.frame_rate.num, parsed.format.frame_rate.den), (30_000, 1_001));
        assert_eq!((parsed.format.sample_rate, parsed.format.channels), (48_000, 2));
        assert_eq!(parsed.seconds, 5);
    }

    #[test]
    fn rejects_bad_declarations() {
        for bad in [
            &["--size", "640x360"][..],
            &["--endpoint", "e"][..],
            &["--endpoint", "e", "--size", "640"][..],
            &["--endpoint", "e", "--size", "640x360", "--fps", "0"][..],
            &["--endpoint", "e", "--size", "640x360", "--fps", "30/0"][..],
            &["--endpoint", "e", "--audio", "48000"][..],
            &["--endpoint", "e", "--size", "64x64", "--seconds", "0"][..],
            &["--endpoint", "e", "--size", "64x64", "--seconds", "99999"][..],
            &["--endpoint", "e", "--size", "64x64", "--bogus"][..],
            &["--endpoint"][..],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?} should be rejected");
        }
    }
}
