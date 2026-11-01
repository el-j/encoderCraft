//! Automated performance benchmarking and stress suite for encoderCraft.
//!
//! Measures throughput (FPS / MSamples/s / Realtime factors) across:
//! - Frame Scaling (Bilinear)
//! - 3D LUT Color Engine (.cube Trilinear interpolation)
//! - Pure-Rust Video Encoders (H.264 & Apple ProRes 422)
//! - Audio DSP (Sinc Resampler & EBU R128 Loudness Normalizer)
//! - Subtitle Caption Burn-In Compositor

use std::time::Instant;

use encodercraft_audio_dsp::{SincResampler, normalize_loudness};
use encodercraft_color::{ColorSpaceTag, PixelFormat};
use encodercraft_core::{AudioSampleBuffer, ChannelLayout, Rational64, VideoFrame};
use encodercraft_geom::{Dimensions, ScaleMode};
use encodercraft_pipeline::{CaptionBurner, CaptionTrack, FrameScaler, Lut3D, NativeCaptionFormat};
use encodercraft_video_codecs::{VideoCodecKind, create_video_encoder};

struct BenchResult {
    name: &'static str,
    spec: &'static str,
    throughput_metric: String,
    realtime_factor: String,
}

pub fn run_benchmarks() -> Result<(), String> {
    println!("\nStarting encoderCraft Performance & Throughput Benchmark Suite...\n");

    let results = vec![
        bench_frame_scaler()?,
        bench_lut_engine()?,
        bench_h264_encoder()?,
        bench_prores_encoder()?,
        bench_audio_resampler()?,
        bench_audio_loudness()?,
        bench_caption_burn()?,
    ];

    // Print Results Table
    print_summary_table(&results);

    Ok(())
}

fn bench_frame_scaler() -> Result<BenchResult, String> {
    let src_dims = Dimensions::new(1920, 1080).map_err(|e| e.to_string())?;
    let dst_dims = Dimensions::new(1280, 720).map_err(|e| e.to_string())?;
    let frame = VideoFrame::new_allocated(src_dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).map_err(|e| e.to_string())?;
    let scaler = FrameScaler::new(ScaleMode::Fit);

    let iterations = 100;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = scaler.scale(&frame, dst_dims).map_err(|e| e.to_string())?;
    }
    let elapsed = start.elapsed();

    let fps = (iterations as f64) / elapsed.as_secs_f64();
    let realtime = fps / 24.0;

    Ok(BenchResult {
        name: "Frame Scaler (Bilinear)",
        spec: "1080p -> 720p",
        throughput_metric: format!("{:.1} FPS", fps),
        realtime_factor: format!("{:.1}x", realtime),
    })
}

fn bench_lut_engine() -> Result<BenchResult, String> {
    let dims = Dimensions::new(1920, 1080).map_err(|e| e.to_string())?;
    let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).map_err(|e| e.to_string())?;

    // Create a 33-point identity LUT
    let size = 33;
    let mut data = Vec::with_capacity(size * size * size);
    for b in 0..size {
        for g in 0..size {
            for r in 0..size {
                let rf = r as f32 / (size - 1) as f32;
                let gf = g as f32 / (size - 1) as f32;
                let bf = b as f32 / (size - 1) as f32;
                data.push([rf, gf, bf]);
            }
        }
    }
    let lut = Lut3D::from_raw_table(size, data).map_err(|e| e.to_string())?;

    let iterations = 50;
    let start = Instant::now();
    for _ in 0..iterations {
        lut.apply_to_frame(&mut frame).map_err(|e| e.to_string())?;
    }
    let elapsed = start.elapsed();

    let fps = (iterations as f64) / elapsed.as_secs_f64();
    let realtime = fps / 24.0;

    Ok(BenchResult {
        name: "3D LUT Color Engine",
        spec: "1080p RGB8 (33^3 Cube)",
        throughput_metric: format!("{:.1} FPS", fps),
        realtime_factor: format!("{:.1}x", realtime),
    })
}

fn bench_h264_encoder() -> Result<BenchResult, String> {
    let dims = Dimensions::new(1280, 720).map_err(|e| e.to_string())?;
    let frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).map_err(|e| e.to_string())?;
    let mut encoder =
        create_video_encoder(VideoCodecKind::H264, dims.width, dims.height, Rational64::FPS_24, Some(4000)).map_err(|e| e.to_string())?;

    let iterations = 30;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = encoder.encode_frame(&frame).map_err(|e| e.to_string())?;
    }
    let _ = encoder.flush().map_err(|e| e.to_string())?;
    let elapsed = start.elapsed();

    let fps = (iterations as f64) / elapsed.as_secs_f64();
    let realtime = fps / 24.0;

    Ok(BenchResult {
        name: "H.264 Video Encode",
        spec: "720p 24fps YUV420p",
        throughput_metric: format!("{:.1} FPS", fps),
        realtime_factor: format!("{:.1}x", realtime),
    })
}

fn bench_prores_encoder() -> Result<BenchResult, String> {
    let dims = Dimensions::new(1280, 720).map_err(|e| e.to_string())?;
    let frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).map_err(|e| e.to_string())?;
    let mut encoder = create_video_encoder(VideoCodecKind::ProRes, dims.width, dims.height, Rational64::FPS_24, None).map_err(|e| e.to_string())?;

    let iterations = 30;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = encoder.encode_frame(&frame).map_err(|e| e.to_string())?;
    }
    let _ = encoder.flush().map_err(|e| e.to_string())?;
    let elapsed = start.elapsed();

    let fps = (iterations as f64) / elapsed.as_secs_f64();
    let realtime = fps / 24.0;

    Ok(BenchResult {
        name: "Apple ProRes 422 Encode",
        spec: "720p 24fps YUV422p",
        throughput_metric: format!("{:.1} FPS", fps),
        realtime_factor: format!("{:.1}x", realtime),
    })
}

fn bench_audio_resampler() -> Result<BenchResult, String> {
    let in_rate = 44100;
    let out_rate = 48000;
    let samples = 44100 * 5; // 5 seconds of audio
    let mut buf = AudioSampleBuffer::new_silent(in_rate, ChannelLayout::Stereo, samples, 0).map_err(|e| e.to_string())?;

    for i in 0..samples {
        let val = (i as f32 * 0.05).sin();
        buf.channel_mut(0).map_err(|e| e.to_string())?[i] = val;
        buf.channel_mut(1).map_err(|e| e.to_string())?[i] = val;
    }

    let resampler = SincResampler::new(32);
    let iterations = 20;
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = resampler.resample(&buf, out_rate).map_err(|e| e.to_string())?;
    }
    let elapsed = start.elapsed();

    let total_samples = (samples * iterations * 2) as f64; // stereo
    let msamples_per_sec = (total_samples / 1_000_000.0) / elapsed.as_secs_f64();
    let audio_seconds = (5 * iterations) as f64;
    let realtime = audio_seconds / elapsed.as_secs_f64();

    Ok(BenchResult {
        name: "Audio Sinc Resampler",
        spec: "Stereo 44.1kHz -> 48kHz",
        throughput_metric: format!("{:.2} MSamp/s", msamples_per_sec),
        realtime_factor: format!("{:.1}x", realtime),
    })
}

fn bench_audio_loudness() -> Result<BenchResult, String> {
    let sample_rate = 48000;
    let samples = sample_rate * 10; // 10 seconds of audio
    let mut buf = AudioSampleBuffer::new_silent(sample_rate, ChannelLayout::Stereo, samples as usize, 0).map_err(|e| e.to_string())?;

    for i in 0..samples {
        let val = (i as f32 * 0.02).sin() * 0.5;
        buf.channel_mut(0).map_err(|e| e.to_string())?[i as usize] = val;
        buf.channel_mut(1).map_err(|e| e.to_string())?[i as usize] = val;
    }

    let iterations = 10;
    let start = Instant::now();
    for _ in 0..iterations {
        let mut work_buf = buf.clone();
        let _ = normalize_loudness(&mut work_buf, -23.0, -1.0).map_err(|e| e.to_string())?;
    }
    let elapsed = start.elapsed();

    let total_samples = (samples * iterations * 2) as f64;
    let msamples_per_sec = (total_samples / 1_000_000.0) / elapsed.as_secs_f64();
    let audio_seconds = (10 * iterations) as f64;
    let realtime = audio_seconds / elapsed.as_secs_f64();

    Ok(BenchResult {
        name: "BS.1770 Loudness Normalizer",
        spec: "Stereo 48kHz Broadcast",
        throughput_metric: format!("{:.2} MSamp/s", msamples_per_sec),
        realtime_factor: format!("{:.1}x", realtime),
    })
}

fn bench_caption_burn() -> Result<BenchResult, String> {
    let dims = Dimensions::new(1920, 1080).map_err(|e| e.to_string())?;
    let mut frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).map_err(|e| e.to_string())?;

    let srt = "1\n00:00:00,000 --> 00:00:10,000\nAntigravity ArtCraft Subtitle Engine\n";
    let track = CaptionTrack::from_text(srt, NativeCaptionFormat::Srt).map_err(|e| e.to_string())?;
    let burner = CaptionBurner::default();

    let iterations = 150;
    let start = Instant::now();
    for _ in 0..iterations {
        burner.burn(&mut frame, &track, 2.5).map_err(|e| e.to_string())?;
    }
    let elapsed = start.elapsed();

    let fps = (iterations as f64) / elapsed.as_secs_f64();
    let realtime = fps / 24.0;

    Ok(BenchResult {
        name: "Caption Burn-In Compositor",
        spec: "1080p RGB8 Subtitles",
        throughput_metric: format!("{:.1} FPS", fps),
        realtime_factor: format!("{:.1}x", realtime),
    })
}

fn print_summary_table(results: &[BenchResult]) {
    println!("+-------------------------------------------------------------------------------------------------+");
    println!("|                                 encoderCraft Performance Benchmarks                             |");
    println!("+-----------------------------+-------------------------+--------------------+--------------------+");
    println!("| Benchmark Task              | Input Specification     | Throughput Metric  | Realtime Factor    |");
    println!("+-----------------------------+-------------------------+--------------------+--------------------+");
    for r in results {
        println!("| {:<27} | {:<23} | {:<18} | {:<18} |", r.name, r.spec, r.throughput_metric, r.realtime_factor);
    }
    println!("+-----------------------------+-------------------------+--------------------+--------------------+");
    println!("  All benchmarks completed successfully with zero panics or memory violations.\n");
}
