//! Background frame decoder for the preview monitor.
//!
//! Decoding runs on its own thread so scrubbing and playback never block the UI. Requests are
//! coalesced: only the newest requested frame is decoded.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use encodercraft_source::{MediaProbe, MediaReader};

/// Widest preview frame handed to the UI; larger sources are box-downscaled.
const MAX_PREVIEW_WIDTH: usize = 960;

enum Request {
    Frame(u64),
}

/// Result messages delivered to the UI thread.
pub enum PreviewEvent {
    /// The source opened; carries its probed facts and frame count.
    Opened { probe: Box<MediaProbe>, frame_count: u64 },
    /// A decoded frame as straight-alpha RGBA8 at `width` x `height`.
    Frame { index: u64, width: usize, height: usize, rgba: Vec<u8> },
    /// Opening or decoding failed; the message is user presentable.
    Failed(String),
}

/// Handle to the decoder thread of one source file.
pub struct PreviewDecoder {
    path: PathBuf,
    requests: Sender<Request>,
    events: Receiver<PreviewEvent>,
}

impl std::fmt::Debug for PreviewDecoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreviewDecoder").field("path", &self.path).finish()
    }
}

impl PreviewDecoder {
    /// Starts decoding `path` on a background thread.
    pub fn open(path: PathBuf) -> Self {
        let (req_tx, req_rx) = channel::<Request>();
        let (ev_tx, ev_rx) = channel::<PreviewEvent>();
        let worker_path = path.clone();
        let spawned = std::thread::Builder::new().name("encodercraft-preview".into()).spawn({
            let ev_tx = ev_tx.clone();
            move || run_worker(&worker_path, &req_rx, &ev_tx)
        });
        if spawned.is_err() {
            let _ = ev_tx.send(PreviewEvent::Failed("could not start the preview decoder thread".into()));
        }
        Self { path, requests: req_tx, events: ev_rx }
    }

    /// The file this decoder reads.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Asks for the frame at `index`; superseded by any newer request.
    pub fn request_frame(&self, index: u64) {
        let _ = self.requests.send(Request::Frame(index));
    }

    /// Returns the next pending event, if any.
    pub fn poll(&self) -> Option<PreviewEvent> {
        self.events.try_recv().ok()
    }
}

fn run_worker(path: &std::path::Path, requests: &Receiver<Request>, events: &Sender<PreviewEvent>) {
    let reader = match MediaReader::open(path) {
        Ok(r) => r,
        Err(e) => {
            let _ = events.send(PreviewEvent::Failed(e.to_string()));
            return;
        }
    };
    let frame_count = reader.frame_count();
    if events.send(PreviewEvent::Opened { probe: Box::new(reader.probe().clone()), frame_count }).is_err() {
        return;
    }

    while let Ok(Request::Frame(mut index)) = requests.recv() {
        // Coalesce: skip straight to the newest queued request.
        while let Ok(Request::Frame(newer)) = requests.try_recv() {
            index = newer;
        }
        let event = match reader.frame_at_index(index) {
            Ok(frame) => {
                let (w, h) = (frame.dimensions.width as usize, frame.dimensions.height as usize);
                let (width, height, rgba) = downscale(frame.data(), w, h);
                PreviewEvent::Frame { index, width, height, rgba }
            }
            Err(e) => PreviewEvent::Failed(e.to_string()),
        };
        if events.send(event).is_err() {
            return;
        }
    }
}

/// Box-filters an RGBA8 image down to at most [`MAX_PREVIEW_WIDTH`] wide.
fn downscale(src: &[u8], w: usize, h: usize) -> (usize, usize, Vec<u8>) {
    if w == 0 || h == 0 || src.len() < w.saturating_mul(h).saturating_mul(4) {
        return (0, 0, Vec::new());
    }
    if w <= MAX_PREVIEW_WIDTH {
        return (w, h, src.to_vec());
    }
    let step = w.div_ceil(MAX_PREVIEW_WIDTH);
    let (ow, oh) = (w / step, h / step);
    let mut out = Vec::with_capacity(ow * oh * 4);
    for oy in 0..oh {
        for ox in 0..ow {
            let mut acc = [0u32; 4];
            for dy in 0..step {
                let row = (oy * step + dy) * w;
                for dx in 0..step {
                    let i = (row + ox * step + dx) * 4;
                    if let Some(px) = src.get(i..i + 4) {
                        for (a, v) in acc.iter_mut().zip(px) {
                            *a += u32::from(*v);
                        }
                    }
                }
            }
            let n = (step * step) as u32;
            out.extend(acc.iter().map(|a| (a / n) as u8));
        }
    }
    (ow, oh, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn write_bmp(path: &std::path::Path, w: u32, h: u32) {
        let row = (w * 3).div_ceil(4) * 4;
        let data = row * h;
        let mut b = Vec::new();
        b.extend_from_slice(b"BM");
        b.extend_from_slice(&(54 + data).to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&54u32.to_le_bytes());
        b.extend_from_slice(&40u32.to_le_bytes());
        b.extend_from_slice(&w.to_le_bytes());
        b.extend_from_slice(&h.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&24u16.to_le_bytes());
        b.extend_from_slice(&[0; 4]);
        b.extend_from_slice(&data.to_le_bytes());
        b.extend_from_slice(&[0; 16]);
        for y in 0..h {
            for x in 0..row {
                b.push((x * 7 + y * 3) as u8);
            }
        }
        std::fs::write(path, b).unwrap();
    }

    fn wait_for(decoder: &PreviewDecoder, mut pred: impl FnMut(&PreviewEvent) -> bool) -> PreviewEvent {
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if let Some(ev) = decoder.poll() {
                if pred(&ev) {
                    return ev;
                }
            } else {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        panic!("timed out waiting for preview event");
    }

    #[test]
    fn decodes_real_pixels_from_a_still() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("still.bmp");
        write_bmp(&path, 32, 20);
        let decoder = PreviewDecoder::open(path);
        let ev = wait_for(&decoder, |e| matches!(e, PreviewEvent::Opened { .. }));
        let PreviewEvent::Opened { frame_count, probe } = ev else { panic!("expected Opened") };
        assert_eq!(frame_count, 1);
        assert_eq!(probe.video.as_ref().map(|v| (v.width, v.height)), Some((32, 20)));

        decoder.request_frame(0);
        let ev = wait_for(&decoder, |e| matches!(e, PreviewEvent::Frame { .. }));
        let PreviewEvent::Frame { width, height, rgba, .. } = ev else { panic!("expected Frame") };
        assert_eq!((width, height), (32, 20));
        assert_eq!(rgba.len(), 32 * 20 * 4);
        assert!(rgba.as_chunks::<4>().0.iter().any(|p| p[0] != rgba[0] || p[1] != rgba[1]), "frame must not be flat");
    }

    #[test]
    fn reports_an_error_for_unreadable_sources() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("junk.mov");
        std::fs::write(&path, b"not media").unwrap();
        let decoder = PreviewDecoder::open(path);
        let ev = wait_for(&decoder, |e| matches!(e, PreviewEvent::Failed(_)));
        assert!(matches!(ev, PreviewEvent::Failed(msg) if !msg.is_empty()));
    }

    #[test]
    fn downscale_caps_width_and_averages() {
        let (w, h) = (MAX_PREVIEW_WIDTH * 2, 4);
        let src = vec![200u8; w * h * 4];
        let (ow, oh, out) = downscale(&src, w, h);
        assert_eq!((ow, oh), (MAX_PREVIEW_WIDTH, 2));
        assert!(out.iter().all(|v| *v == 200));
        assert_eq!(downscale(&[], 0, 0).2.len(), 0);
    }
}
