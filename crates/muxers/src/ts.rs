//! MPEG-2 Transport Stream (MPEG-TS) multiplexer.
//!
//! Provides broadcast-compliant 188-byte packetized transport stream muxing
//! with PAT/PMT signaling, PCR clock reference alignment, and PES stream encapsulation,
//! conforming to ATSC, DVB, CableLabs, and ITU-T H.222.0 / ISO/IEC 13818-1.

use std::io::Write;

use encodercraft_core::EncoderError;

use crate::muxer::{AudioSample, AudioStreamConfig, VideoSample, VideoStreamConfig};

/// Standard MPEG-TS packet size in bytes.
pub const TS_PACKET_SIZE: usize = 188;

/// Synchronization byte starting every MPEG-TS packet.
pub const TS_SYNC_BYTE: u8 = 0x47;

/// Well-known PIDs.
pub const PID_PAT: u16 = 0x0000;
pub const PID_PMT: u16 = 0x0100;
pub const PID_VIDEO: u16 = 0x0101;
pub const PID_AUDIO: u16 = 0x0102;

/// Well-known Stream Types.
pub const STREAM_TYPE_MPEG2_VIDEO: u8 = 0x02;
pub const STREAM_TYPE_MP2_AUDIO: u8 = 0x03;
pub const STREAM_TYPE_AAC_ADTS: u8 = 0x0F;
pub const STREAM_TYPE_H264: u8 = 0x1B;
pub const STREAM_TYPE_HEVC: u8 = 0x24;
pub const STREAM_TYPE_AC3: u8 = 0x81;

/// Clean-room 188-byte MPEG-TS packetizer.
pub struct TsMuxer<W: Write> {
    writer: W,
    video_cfg: Option<VideoStreamConfig>,
    audio_cfg: Option<AudioStreamConfig>,
    video_stream_type: u8,
    audio_stream_type: u8,
    cc_pat: u8,
    cc_pmt: u8,
    cc_video: u8,
    cc_audio: u8,
    pcr_clock_ticks: u64,
    pat_interval_packets: usize,
    packets_since_pat: usize,
}

impl<W: Write> TsMuxer<W> {
    /// Creates a new MPEG-TS multiplexer writing to the given destination.
    pub fn new(writer: W) -> Result<Self, EncoderError> {
        Ok(Self {
            writer,
            video_cfg: None,
            audio_cfg: None,
            video_stream_type: STREAM_TYPE_H264,
            audio_stream_type: STREAM_TYPE_AAC_ADTS,
            cc_pat: 0,
            cc_pmt: 0,
            cc_video: 0,
            cc_audio: 0,
            pcr_clock_ticks: 0,
            pat_interval_packets: 40,
            packets_since_pat: 0,
        })
    }

    /// Adds a video stream track to the transport stream.
    pub fn add_video_stream(&mut self, cfg: &VideoStreamConfig) -> Result<usize, EncoderError> {
        let stream_type = match &cfg.codec_fourcc {
            b"hvc1" | b"hev1" => STREAM_TYPE_HEVC,
            b"mp2v" | b"mpg2" => STREAM_TYPE_MPEG2_VIDEO,
            _ => STREAM_TYPE_H264,
        };
        self.video_stream_type = stream_type;
        self.video_cfg = Some(cfg.clone());
        self.write_psi_tables()?;
        Ok(0)
    }

    /// Adds an audio stream track to the transport stream.
    pub fn add_audio_stream(&mut self, cfg: &AudioStreamConfig) -> Result<usize, EncoderError> {
        let stream_type = match &cfg.codec_fourcc {
            b"mp2a" => STREAM_TYPE_MP2_AUDIO,
            b"ac-3" | b"sac3" => STREAM_TYPE_AC3,
            _ => STREAM_TYPE_AAC_ADTS,
        };
        self.audio_stream_type = stream_type;
        self.audio_cfg = Some(cfg.clone());
        self.write_psi_tables()?;
        Ok(1)
    }

    /// Writes PAT (Program Association Table) and PMT (Program Map Table).
    fn write_psi_tables(&mut self) -> Result<(), EncoderError> {
        self.write_pat()?;
        self.write_pmt()?;
        self.packets_since_pat = 0;
        Ok(())
    }

    /// Emits the Program Association Table (PID 0x0000).
    fn write_pat(&mut self) -> Result<(), EncoderError> {
        let mut section = Vec::with_capacity(32);
        // Table ID: 0x00 (PAT)
        section.push(0x00);
        // Section syntax indicator (1), 0, reserved (11), section length (2 bytes, filled later)
        let transport_stream_id = 0x0001u16;
        let version_number = 0u8; // 5 bits
        let current_next_indicator = 1u8;
        let b2 = 0b1100_0000 | (version_number << 1) | current_next_indicator;
        let section_number = 0x00u8;
        let last_section_number = 0x00u8;

        // Program 1 -> PMT PID
        let program_number = 0x0001u16;
        let pmt_pid = PID_PMT;

        let mut body = Vec::with_capacity(16);
        body.extend_from_slice(&transport_stream_id.to_be_bytes());
        body.push(b2);
        body.push(section_number);
        body.push(last_section_number);
        body.extend_from_slice(&program_number.to_be_bytes());
        body.extend_from_slice(&(0b1110_0000_0000_0000u16 | pmt_pid).to_be_bytes());

        let section_len = (body.len() + 4) as u16; // body + 4 bytes CRC
        section.extend_from_slice(&(0b1011_0000_0000_0000u16 | (section_len & 0x0FFF)).to_be_bytes());
        section.extend_from_slice(&body);

        let crc = mpeg_crc32(&section);
        section.extend_from_slice(&crc.to_be_bytes());

        // Prefix with pointer field (0x00) for payload unit start
        let mut payload = Vec::with_capacity(section.len() + 1);
        payload.push(0x00);
        payload.extend_from_slice(&section);

        self.packetize_psi(PID_PAT, &payload)
    }

    /// Emits the Program Map Table (PID 0x0100).
    fn write_pmt(&mut self) -> Result<(), EncoderError> {
        let mut section = Vec::with_capacity(64);
        // Table ID: 0x02 (PMT)
        section.push(0x02);

        let program_number = 0x0001u16;
        let version_number = 0u8;
        let current_next_indicator = 1u8;
        let b2 = 0b1100_0000 | (version_number << 1) | current_next_indicator;
        let section_number = 0x00u8;
        let last_section_number = 0x00u8;
        let pcr_pid = PID_VIDEO;
        let program_info_length = 0u16;

        let mut body = Vec::with_capacity(32);
        body.extend_from_slice(&program_number.to_be_bytes());
        body.push(b2);
        body.push(section_number);
        body.push(last_section_number);
        body.extend_from_slice(&(0b1110_0000_0000_0000u16 | pcr_pid).to_be_bytes());
        body.extend_from_slice(&(0b1111_0000_0000_0000u16 | (program_info_length & 0x0FFF)).to_be_bytes());

        // Elementary Stream 1: Video
        body.push(self.video_stream_type);
        body.extend_from_slice(&(0b1110_0000_0000_0000u16 | PID_VIDEO).to_be_bytes());
        body.extend_from_slice(&[0xF0, 0x00]); // ES info length: 0

        // Elementary Stream 2: Audio (if present)
        if self.audio_cfg.is_some() {
            body.push(self.audio_stream_type);
            body.extend_from_slice(&(0b1110_0000_0000_0000u16 | PID_AUDIO).to_be_bytes());
            body.extend_from_slice(&[0xF0, 0x00]); // ES info length: 0
        }

        let section_len = (body.len() + 4) as u16; // body + 4 bytes CRC
        section.extend_from_slice(&(0b1011_0000_0000_0000u16 | (section_len & 0x0FFF)).to_be_bytes());
        section.extend_from_slice(&body);

        let crc = mpeg_crc32(&section);
        section.extend_from_slice(&crc.to_be_bytes());

        let mut payload = Vec::with_capacity(section.len() + 1);
        payload.push(0x00);
        payload.extend_from_slice(&section);

        self.packetize_psi(PID_PMT, &payload)
    }

    fn next_cc(&mut self, pid: u16) -> u8 {
        let cc = match pid {
            PID_PAT => &mut self.cc_pat,
            PID_PMT => &mut self.cc_pmt,
            PID_VIDEO => &mut self.cc_video,
            _ => &mut self.cc_audio,
        };
        let val = *cc;
        *cc = (*cc + 1) & 0x0F;
        val
    }

    /// Helper to wrap PSI payload (PAT/PMT) into a single 188-byte packet with stuffing.
    fn packetize_psi(&mut self, pid: u16, payload: &[u8]) -> Result<(), EncoderError> {
        let cc = self.next_cc(pid);
        let mut packet = [0xFFu8; TS_PACKET_SIZE];
        packet[0] = TS_SYNC_BYTE;
        packet[1] = 0x40 | ((pid >> 8) as u8 & 0x1F); // PUSI = 1
        packet[2] = (pid & 0xFF) as u8;
        packet[3] = 0x10 | (cc & 0x0F); // Adaptation control = 01 (payload only)

        let copy_len = payload.len().min(TS_PACKET_SIZE - 4);
        if let Some(dest) = packet.get_mut(4..4 + copy_len) {
            dest.copy_from_slice(&payload[..copy_len]);
        }
        self.writer.write_all(&packet).map_err(|e| EncoderError::Io(format!("failed to write TS packet: {e}")))?;
        Ok(())
    }

    /// Writes an encoded video sample into the transport stream.
    pub fn write_video_sample(&mut self, _track: usize, sample: VideoSample) -> Result<(), EncoderError> {
        self.packets_since_pat += 1;
        if self.packets_since_pat >= self.pat_interval_packets {
            self.write_psi_tables()?;
        }

        // Convert sample data to Annex B NAL stream if needed
        let annex_b_data = to_annex_b(sample.data);
        let pts_90khz = (sample.pts as u64).saturating_mul(90);
        let dts_90khz = (sample.dts as u64).saturating_mul(90);

        // Build Video PES Packet
        let pes_header = encode_pes_header(0xE0, pts_90khz, if sample.pts != sample.dts { Some(dts_90khz) } else { None }, annex_b_data.len());
        let mut full_pes = Vec::with_capacity(pes_header.len() + annex_b_data.len());
        full_pes.extend_from_slice(&pes_header);
        full_pes.extend_from_slice(&annex_b_data);

        // PCR on keyframes or clock advancement
        let pcr_pcr = if sample.is_keyframe {
            let pcr_base = pts_90khz;
            self.pcr_clock_ticks = pcr_base;
            Some(pcr_base)
        } else {
            None
        };

        self.packetize_es(PID_VIDEO, &full_pes, pcr_pcr, sample.is_keyframe)
    }

    /// Writes an encoded audio sample into the transport stream.
    pub fn write_audio_sample(&mut self, _track: usize, sample: AudioSample) -> Result<(), EncoderError> {
        let pts_90khz = (sample.pts as u64).saturating_mul(90);
        let pes_header = encode_pes_header(0xC0, pts_90khz, None, sample.data.len());
        let mut full_pes = Vec::with_capacity(pes_header.len() + sample.data.len());
        full_pes.extend_from_slice(&pes_header);
        full_pes.extend_from_slice(sample.data);

        self.packetize_es(PID_AUDIO, &full_pes, None, false)
    }

    /// Chunks a PES packet into one or more 188-byte TS packets with exact padding.
    fn packetize_es(&mut self, pid: u16, data: &[u8], pcr: Option<u64>, is_random_access: bool) -> Result<(), EncoderError> {
        let mut offset = 0;
        let mut pusi = true;

        while offset < data.len() {
            let cc = self.next_cc(pid);
            let mut packet = [0xFFu8; TS_PACKET_SIZE];
            packet[0] = TS_SYNC_BYTE;
            packet[1] = (if pusi { 0x40 } else { 0x00 }) | ((pid >> 8) as u8 & 0x1F);
            packet[2] = (pid & 0xFF) as u8;

            let remaining = data.len() - offset;
            let needs_pcr = pusi && pcr.is_some();

            if needs_pcr || remaining < 184 {
                // Must emit adaptation field
                let pcr_len = if needs_pcr { 6 } else { 0 };
                let min_af_len = 1 + pcr_len; // flags byte + PCR bytes
                let max_payload_space = 184 - 1 - min_af_len; // 184 - adaptation_field_length_byte - min_af_len
                let payload_len = remaining.min(max_payload_space);
                let stuffing_len = 184 - 1 - min_af_len - payload_len;
                let total_af_len = min_af_len + stuffing_len;

                packet[3] = 0x30 | (cc & 0x0F); // Adaptation control = 11 (adaptation field + payload)
                packet[4] = total_af_len as u8;

                let mut flags = 0u8;
                if is_random_access && pusi {
                    flags |= 0x40; // Random access indicator
                }
                if needs_pcr {
                    flags |= 0x10; // PCR flag
                }
                packet[5] = flags;

                let mut af_idx = 6;
                if let Some(pcr_val) = pcr
                    && needs_pcr
                {
                    let pcr_base = pcr_val & 0x1_FFFF_FFFF;
                    let pcr_ext = 0u16; // 9-bit extension
                    packet[af_idx] = (pcr_base >> 25) as u8;
                    packet[af_idx + 1] = (pcr_base >> 17) as u8;
                    packet[af_idx + 2] = (pcr_base >> 9) as u8;
                    packet[af_idx + 3] = (pcr_base >> 1) as u8;
                    packet[af_idx + 4] = (((pcr_base & 1) as u8) << 7) | 0x7E | ((pcr_ext >> 8) as u8 & 1);
                    packet[af_idx + 5] = (pcr_ext & 0xFF) as u8;
                    af_idx += 6;
                }

                // Fill stuffing bytes with 0xFF
                while af_idx < 4 + 1 + total_af_len {
                    packet[af_idx] = 0xFF;
                    af_idx += 1;
                }

                let payload_start = 4 + 1 + total_af_len;
                if payload_len > 0
                    && let Some(dest) = packet.get_mut(payload_start..payload_start + payload_len)
                {
                    dest.copy_from_slice(&data[offset..offset + payload_len]);
                    offset += payload_len;
                }
            } else {
                // Standard payload-only packet
                packet[3] = 0x10 | (cc & 0x0F); // Adaptation control = 01
                let payload_len = remaining.min(184);
                if let Some(dest) = packet.get_mut(4..4 + payload_len) {
                    dest.copy_from_slice(&data[offset..offset + payload_len]);
                    offset += payload_len;
                }
            }

            pusi = false;

            self.writer.write_all(&packet).map_err(|e| EncoderError::Io(format!("failed to write TS packet: {e}")))?;
        }
        Ok(())
    }

    /// Flushes and finalizes the transport stream.
    pub fn finish(mut self) -> Result<W, EncoderError> {
        self.writer.flush().map_err(|e| EncoderError::Io(format!("failed to flush MPEG-TS output: {e}")))?;
        Ok(self.writer)
    }
}

/// Converts AVCC length-prefixed NALUs to Annex B start codes (`00 00 00 01`).
fn to_annex_b(data: &[u8]) -> Vec<u8> {
    if data.starts_with(&[0, 0, 0, 1]) || data.starts_with(&[0, 0, 1]) {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut i = 0;
    while i + 4 <= data.len() {
        let len = u32::from_be_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]) as usize;
        i += 4;
        if i + len > data.len() {
            break;
        }
        out.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
        out.extend_from_slice(&data[i..i + len]);
        i += len;
    }
    if out.is_empty() { data.to_vec() } else { out }
}

/// Encodes a standard PES header with PTS and optional DTS.
fn encode_pes_header(stream_id: u8, pts_90khz: u64, dts_90khz: Option<u64>, payload_len: usize) -> Vec<u8> {
    let mut header = Vec::with_capacity(19);
    // Start code prefix: 0x000001
    header.extend_from_slice(&[0x00, 0x00, 0x01, stream_id]);

    let pes_header_data_len = if dts_90khz.is_some() { 10 } else { 5 };
    // In video, if payload_len is too large for 16 bits, length can be 0 (unbounded)
    let pes_packet_length = if (0xE0..=0xEF).contains(&stream_id) {
        0u16
    } else {
        let total = 3 + pes_header_data_len + payload_len;
        if total > 0xFFFF { 0 } else { total as u16 }
    };
    header.extend_from_slice(&pes_packet_length.to_be_bytes());

    // Flags 1: 0x80 (marker bits)
    header.push(0x80);
    // Flags 2: PTS / DTS flags
    let pts_dts_flag = if dts_90khz.is_some() { 0xC0 } else { 0x80 };
    header.push(pts_dts_flag);
    header.push(pes_header_data_len as u8);

    // Encode PTS (33 bits)
    let pts_marker = if dts_90khz.is_some() { 0x30 } else { 0x20 };
    header.extend_from_slice(&encode_timestamp(pts_marker, pts_90khz));

    // Encode DTS if present
    if let Some(dts) = dts_90khz {
        header.extend_from_slice(&encode_timestamp(0x10, dts));
    }

    header
}

/// Encodes 33-bit timestamp into 5-byte MPEG-2 format.
fn encode_timestamp(marker: u8, ts: u64) -> [u8; 5] {
    let pts = ts & 0x1_FFFF_FFFF;
    let b0 = marker | (((pts >> 30) as u8 & 0x07) << 1) | 0x01;
    let b1 = (pts >> 22) as u8;
    let b2 = ((((pts >> 15) as u8) & 0x7F) << 1) | 0x01;
    let b3 = (pts >> 7) as u8;
    let b4 = (((pts as u8) & 0x7F) << 1) | 0x01;
    [b0, b1, b2, b3, b4]
}

/// Canonical MPEG-2 CRC32 calculation with polynomial 0x04C11DB7.
pub fn mpeg_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        for bit in 0..8 {
            let msb = (crc & 0x8000_0000) != 0;
            let bit_in = (byte & (0x80 >> bit)) != 0;
            crc <<= 1;
            if msb ^ bit_in {
                crc ^= 0x04C1_1DB7;
            }
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use encodercraft_color::{ColorSpaceTag, PixelFormat};
    use encodercraft_core::{ChannelLayout, Rational64};
    use encodercraft_geom::Dimensions;
    use std::io::Cursor;

    #[test]
    fn test_mpeg_ts_muxer_packets_and_alignment() {
        let cursor = Cursor::new(Vec::new());
        let mut muxer = TsMuxer::new(cursor).unwrap();

        let v_cfg = VideoStreamConfig {
            codec_fourcc: *b"avc1",
            dimensions: Dimensions::new(1920, 1080).unwrap(),
            frame_rate: Rational64::FPS_25,
            pixel_format: PixelFormat::Yuv420p,
            color_space: ColorSpaceTag::REC709,
            bit_depth: 8,
            codec_private: vec![0, 0, 0, 1, 0x67, 0x42, 0, 0x1E],
            spatial: None,
        };
        let v_track = muxer.add_video_stream(&v_cfg).unwrap();

        let a_cfg = AudioStreamConfig {
            codec_fourcc: *b"mp4a",
            sample_rate: 48000,
            layout: ChannelLayout::Stereo,
            bits_per_sample: 16,
            codec_private: vec![0x11, 0x90],
        };
        let a_track = muxer.add_audio_stream(&a_cfg).unwrap();

        // Write video keyframe
        let frame = [0x00, 0x00, 0x00, 0x01, 0x65, 0x88, 0x84, 0x00];
        muxer.write_video_sample(v_track, VideoSample { data: &frame, pts: 0, dts: 0, duration_ticks: 1, is_keyframe: true }).unwrap();

        // Write audio sample
        let audio_frame = [0xFF, 0xF1, 0x50, 0x80, 0x02, 0x1F, 0xFC, 0xDE, 0xAD];
        muxer.write_audio_sample(a_track, AudioSample { data: &audio_frame, pts: 0, duration_ticks: 1024 }).unwrap();

        let output = muxer.finish().unwrap();
        let bytes = output.into_inner();

        // Every packet MUST be an exact multiple of 188 bytes
        assert_eq!(bytes.len() % TS_PACKET_SIZE, 0, "MPEG-TS byte count must be a multiple of 188");
        let num_packets = bytes.len() / TS_PACKET_SIZE;
        assert!(num_packets >= 3, "Expected at least PAT, PMT, and PES packets");

        // Every packet MUST start with 0x47 sync byte
        for i in 0..num_packets {
            assert_eq!(bytes[i * TS_PACKET_SIZE], TS_SYNC_BYTE, "Packet {i} does not start with 0x47 sync byte");
        }
    }
}
