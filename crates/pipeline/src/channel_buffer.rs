//! Bounded crossbeam-channel ring buffer enforcing backpressure between pipeline stages.

use crossbeam_channel::{Receiver, Sender, bounded};
use encodercraft_core::{EncoderError, VideoFrame};

/// Maximum number of uncompressed video frames buffered in flight (prevents OOM on 4K/8K media).
pub const DEFAULT_RING_BUFFER_CAPACITY: usize = 8;

/// A bounded frame channel maintaining backpressure.
pub struct FrameRingBuffer {
    sender: Sender<VideoFrame>,
    receiver: Receiver<VideoFrame>,
}

impl Default for FrameRingBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_RING_BUFFER_CAPACITY)
    }
}

impl FrameRingBuffer {
    /// Creates a new bounded frame ring buffer with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, receiver) = bounded(capacity.max(1));
        Self { sender, receiver }
    }

    /// Returns the sender handle.
    pub fn sender(&self) -> Sender<VideoFrame> {
        self.sender.clone()
    }

    /// Returns the receiver handle.
    pub fn receiver(&self) -> Receiver<VideoFrame> {
        self.receiver.clone()
    }

    /// Pushes a frame into the bounded channel, blocking with backpressure if the channel is full.
    pub fn push(&self, frame: VideoFrame) -> Result<(), EncoderError> {
        self.sender.send(frame).map_err(|_| EncoderError::Internal("failed to push frame into pipeline ring buffer: channel closed".into()))
    }

    /// Pops a frame from the bounded channel.
    pub fn pop(&self) -> Result<VideoFrame, EncoderError> {
        self.receiver.recv().map_err(|_| EncoderError::Internal("failed to receive frame from pipeline ring buffer: channel closed".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::{ColorSpaceTag, PixelFormat};
    use encodercraft_core::Rational64;
    use encodercraft_geom::Dimensions;

    #[test]
    fn test_ring_buffer_push_pop() {
        let ring = FrameRingBuffer::new(2);
        let dims = Dimensions::new(4, 4).unwrap();
        let frame = VideoFrame::new_allocated(dims, PixelFormat::Rgb8, ColorSpaceTag::SRGB, 0, 1, Rational64::FPS_24).unwrap();

        ring.push(frame.clone()).unwrap();
        let popped = ring.pop().unwrap();
        assert_eq!(popped.dimensions, dims);
    }
}
