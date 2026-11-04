//! Layered graphics intermediate representation, Photoshop PSD/PSB parser and serializer,
//! and native ArtCraft PhotoCraft container format.

use encodercraft_core::EncoderError;
use serde::{Deserialize, Serialize};

/// Blend modes supported across Photoshop PSD and ArtCraft PhotoCraft.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayerBlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

impl LayerBlendMode {
    pub const fn to_psd_key(self) -> &'static [u8; 4] {
        match self {
            Self::Normal => b"norm",
            Self::Multiply => b"mul ",
            Self::Screen => b"scrn",
            Self::Overlay => b"over",
            Self::Darken => b"dark",
            Self::Lighten => b"lite",
            Self::ColorDodge => b"div ",
            Self::ColorBurn => b"burn",
            Self::HardLight => b"hLit",
            Self::SoftLight => b"sLit",
            Self::Difference => b"diff",
            Self::Exclusion => b"smud",
            Self::Hue => b"hue ",
            Self::Saturation => b"sat ",
            Self::Color => b"colr",
            Self::Luminosity => b"lum ",
        }
    }

    pub fn from_psd_key(key: &[u8]) -> Self {
        match key {
            b"mul " => Self::Multiply,
            b"scrn" => Self::Screen,
            b"over" => Self::Overlay,
            b"dark" => Self::Darken,
            b"lite" => Self::Lighten,
            b"div " | b"dodge" => Self::ColorDodge,
            b"burn" => Self::ColorBurn,
            b"hLit" => Self::HardLight,
            b"sLit" => Self::SoftLight,
            b"diff" => Self::Difference,
            b"smud" => Self::Exclusion,
            b"hue " => Self::Hue,
            b"sat " => Self::Saturation,
            b"colr" => Self::Color,
            b"lum " => Self::Luminosity,
            _ => Self::Normal,
        }
    }
}

/// Color modes for layered documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LayeredColorMode {
    Bitmap = 0,
    Grayscale = 1,
    Indexed = 2,
    Rgb = 3,
    Cmyk = 4,
    Multichannel = 7,
    Duotone = 8,
    Lab = 9,
}

impl LayeredColorMode {
    pub const fn from_u16(val: u16) -> Self {
        match val {
            0 => Self::Bitmap,
            1 => Self::Grayscale,
            2 => Self::Indexed,
            4 => Self::Cmyk,
            7 => Self::Multichannel,
            8 => Self::Duotone,
            9 => Self::Lab,
            _ => Self::Rgb,
        }
    }
}

/// A discrete image layer with position, blend mode, opacity, and RGBA pixel samples.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageLayer {
    pub name: String,
    pub top: i32,
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
    pub blend_mode: LayerBlendMode,
    pub opacity: u8,
    pub visible: bool,
    pub rgba_data: Vec<u8>,
}

impl ImageLayer {
    pub fn new(name: impl Into<String>, width: u32, height: u32, rgba_data: Vec<u8>) -> Self {
        Self {
            name: name.into(),
            top: 0,
            left: 0,
            bottom: height as i32,
            right: width as i32,
            blend_mode: LayerBlendMode::Normal,
            opacity: 255,
            visible: true,
            rgba_data,
        }
    }

    pub fn width(&self) -> u32 {
        (self.right.saturating_sub(self.left)).max(0) as u32
    }

    pub fn height(&self) -> u32 {
        (self.bottom.saturating_sub(self.top)).max(0) as u32
    }
}

/// Intermediate Representation for a Multi-Layer Composite Image.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayeredImage {
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub color_mode: LayeredColorMode,
    pub bit_depth: u8,
    pub layers: Vec<ImageLayer>,
    pub composite_rgba: Vec<u8>,
}

impl LayeredImage {
    pub fn new(title: impl Into<String>, width: u32, height: u32) -> Self {
        let size = (width as usize).saturating_mul(height as usize).saturating_mul(4);
        Self {
            title: title.into(),
            width,
            height,
            color_mode: LayeredColorMode::Rgb,
            bit_depth: 8,
            layers: Vec::new(),
            composite_rgba: vec![255; size],
        }
    }

    pub fn add_layer(&mut self, layer: ImageLayer) {
        self.layers.push(layer);
    }

    /// Serializes image into the native ArtCraft `.photocraft` container format.
    pub fn to_photocraft_binary(&self) -> Result<Vec<u8>, EncoderError> {
        let json = serde_json::to_vec(self).map_err(|e| EncoderError::Internal(format!("failed to serialize PhotoCraft to JSON: {e}")))?;
        let mut out = Vec::with_capacity(13 + json.len());
        // ArtCraft binary signature for PhotoCraft: \x89CRAFT\x00\x01P
        out.extend_from_slice(b"\x89CRAFT\x00\x01P");
        let len_bytes =
            u32::try_from(json.len()).map_err(|_| EncoderError::Internal("photocraft payload exceeds 4GB limit".to_string()))?.to_be_bytes();
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(&json);
        Ok(out)
    }

    /// Deserializes image from native ArtCraft `.photocraft` binary format.
    pub fn from_photocraft_binary(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.len() < 13 || !bytes.starts_with(b"\x89CRAFT\x00\x01P") {
            return Err(EncoderError::InvalidContainer("not a valid ArtCraft .photocraft binary file".to_string()));
        }
        let payload_len = u32::from_be_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]) as usize;
        let payload = bytes.get(13..13 + payload_len).ok_or(EncoderError::UnexpectedEof)?;
        serde_json::from_slice(payload).map_err(|e| EncoderError::InvalidContainer(format!("failed to deserialize photocraft JSON payload: {e}")))
    }

    /// Exports layered image into standard Photoshop PSD (version 1) or PSB (version 2) format.
    pub fn to_psd_bytes(&self, is_psb: bool) -> Result<Vec<u8>, EncoderError> {
        let mut psd = Vec::with_capacity(4096 + self.composite_rgba.len());

        // 1. Header (26 bytes)
        psd.extend_from_slice(b"8BPS");
        psd.extend_from_slice(&(if is_psb { 2u16 } else { 1u16 }).to_be_bytes()); // Version
        psd.extend_from_slice(&[0u8; 6]); // Reserved
        psd.extend_from_slice(&4u16.to_be_bytes()); // Channels: RGBA
        psd.extend_from_slice(&self.height.to_be_bytes()); // Height
        psd.extend_from_slice(&self.width.to_be_bytes()); // Width
        psd.extend_from_slice(&(self.bit_depth as u16).to_be_bytes()); // Depth: 8
        psd.extend_from_slice(&(LayeredColorMode::Rgb as u16).to_be_bytes()); // ColorMode: RGB

        // 2. Color Mode Data (Empty for RGB)
        psd.extend_from_slice(&0u32.to_be_bytes());

        // 3. Image Resources (Empty or minimal)
        psd.extend_from_slice(&0u32.to_be_bytes());

        // 4. Layer and Mask Information Section
        let mut layer_info = Vec::new();
        let layer_count = self.layers.len() as i16;
        layer_info.extend_from_slice(&layer_count.to_be_bytes());

        let mut channel_data_blobs = Vec::new();

        // Layer Records
        for layer in &self.layers {
            layer_info.extend_from_slice(&layer.top.to_be_bytes());
            layer_info.extend_from_slice(&layer.left.to_be_bytes());
            layer_info.extend_from_slice(&layer.bottom.to_be_bytes());
            layer_info.extend_from_slice(&layer.right.to_be_bytes());
            layer_info.extend_from_slice(&4u16.to_be_bytes()); // 4 channels

            let lw = layer.width() as usize;
            let lh = layer.height() as usize;
            let channel_len = lw.saturating_mul(lh);

            // Channel info records: Red (0), Green (1), Blue (2), Alpha (-1)
            for ch_id in [0i16, 1i16, 2i16, -1i16] {
                layer_info.extend_from_slice(&ch_id.to_be_bytes());
                // Data length = 2 bytes compression + raw channel bytes
                let len = (channel_len as u32).saturating_add(2);
                if is_psb {
                    layer_info.extend_from_slice(&(len as u64).to_be_bytes());
                } else {
                    layer_info.extend_from_slice(&len.to_be_bytes());
                }
            }

            // Blend mode: 8BIM + key
            layer_info.extend_from_slice(b"8BIM");
            layer_info.extend_from_slice(layer.blend_mode.to_psd_key());
            layer_info.push(layer.opacity);
            layer_info.push(0); // Clipping: base
            layer_info.push(if layer.visible { 0 } else { 2 }); // Flags: bit 1 = hidden
            layer_info.push(0); // Filler

            // Extra data length
            let name_bytes = layer.name.as_bytes();
            let name_len = name_bytes.len().min(255) as u8;
            let pascal_len = 1 + (name_len as usize);
            let padded_len = (pascal_len + 3) & !3; // Pad to multiple of 4
            let extra_len = 4 + 4 + padded_len; // mask len(4) + blend ranges len(4) + name

            layer_info.extend_from_slice(&(extra_len as u32).to_be_bytes());
            layer_info.extend_from_slice(&0u32.to_be_bytes()); // Layer mask length
            layer_info.extend_from_slice(&0u32.to_be_bytes()); // Layer blending ranges length

            // Pascal string layer name padded
            layer_info.push(name_len);
            layer_info.extend_from_slice(&name_bytes[..name_len as usize]);
            layer_info.resize(layer_info.len() + (padded_len - pascal_len), 0);

            // De-interleave RGBA into planar channels with compression = 0 (Raw)
            let mut r_ch = vec![0u8; 2 + channel_len];
            let mut g_ch = vec![0u8; 2 + channel_len];
            let mut b_ch = vec![0u8; 2 + channel_len];
            let mut a_ch = vec![0u8; 2 + channel_len];

            let chunks = layer.rgba_data.as_chunks::<4>().0;
            for (idx, chunk) in chunks.iter().enumerate().take(channel_len) {
                r_ch[2 + idx] = chunk[0];
                g_ch[2 + idx] = chunk[1];
                b_ch[2 + idx] = chunk[2];
                a_ch[2 + idx] = chunk[3];
            }

            channel_data_blobs.push(r_ch);
            channel_data_blobs.push(g_ch);
            channel_data_blobs.push(b_ch);
            channel_data_blobs.push(a_ch);
        }

        // Write Channel Data
        for blob in channel_data_blobs {
            layer_info.extend_from_slice(&blob);
        }

        let layer_section_len = layer_info.len() as u32;
        if is_psb {
            psd.extend_from_slice(&(layer_section_len as u64).to_be_bytes());
        } else {
            psd.extend_from_slice(&layer_section_len.to_be_bytes());
        }
        psd.extend_from_slice(&layer_info);

        // 5. Composite Image Data Section
        psd.extend_from_slice(&0u16.to_le_bytes()); // Compression: 0 = Raw
        let num_pixels = (self.width as usize).saturating_mul(self.height as usize);

        for ch in 0..4 {
            for idx in 0..num_pixels {
                let byte = self.composite_rgba.get((idx * 4) + ch).copied().unwrap_or(255);
                psd.push(byte);
            }
        }

        Ok(psd)
    }

    /// Parses Photoshop PSD (version 1) or PSB (version 2) file bytes.
    pub fn from_psd_bytes(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.len() < 26 || !bytes.starts_with(b"8BPS") {
            return Err(EncoderError::InvalidContainer("not a Photoshop PSD/PSB file (missing 8BPS header)".to_string()));
        }

        let version = u16::from_be_bytes([bytes[4], bytes[5]]);
        if version != 1 && version != 2 {
            return Err(EncoderError::UnsupportedCodec(format!("unsupported PSD version: {version}")));
        }
        let is_psb = version == 2;

        let height = u32::from_be_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]);
        let width = u32::from_be_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]);
        let bit_depth = bytes[22];
        let color_mode = LayeredColorMode::from_u16(u16::from_be_bytes([bytes[24], bytes[25]]));

        let mut image = Self::new("Imported Photoshop Document", width, height);
        image.color_mode = color_mode;
        image.bit_depth = bit_depth;

        let mut offset = 26;

        // Skip Color Mode Data Section
        if offset + 4 <= bytes.len() {
            let color_mode_len = u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
            offset += 4 + color_mode_len;
        }

        // Skip Image Resources Section
        if offset + 4 <= bytes.len() {
            let res_len = u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
            offset += 4 + res_len;
        }

        // Layer and Mask Information Section
        if (is_psb && offset + 8 <= bytes.len()) || (!is_psb && offset + 4 <= bytes.len()) {
            let layer_section_len = if is_psb {
                let len = u64::from_be_bytes([
                    bytes[offset],
                    bytes[offset + 1],
                    bytes[offset + 2],
                    bytes[offset + 3],
                    bytes[offset + 4],
                    bytes[offset + 5],
                    bytes[offset + 6],
                    bytes[offset + 7],
                ]);
                offset += 8;
                len as usize
            } else {
                let len = u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]);
                offset += 4;
                len as usize
            };

            if layer_section_len >= 2 && offset + 2 <= bytes.len() {
                let layer_count = i16::from_be_bytes([bytes[offset], bytes[offset + 1]]).unsigned_abs() as usize;
                offset += 2;

                for layer_idx in 0..layer_count {
                    if offset + 16 > bytes.len() {
                        break;
                    }
                    let top = i32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]);
                    let left = i32::from_be_bytes([bytes[offset + 4], bytes[offset + 5], bytes[offset + 6], bytes[offset + 7]]);
                    let bottom = i32::from_be_bytes([bytes[offset + 8], bytes[offset + 9], bytes[offset + 10], bytes[offset + 11]]);
                    let right = i32::from_be_bytes([bytes[offset + 12], bytes[offset + 13], bytes[offset + 14], bytes[offset + 15]]);
                    offset += 16;

                    if offset + 2 > bytes.len() {
                        break;
                    }
                    let num_channels = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
                    offset += 2;

                    // Channel records (6 bytes each for PSD, 10 bytes each for PSB)
                    let ch_record_len = if is_psb { 10 } else { 6 };
                    offset += num_channels.saturating_mul(ch_record_len);

                    if offset + 12 > bytes.len() {
                        break;
                    }
                    // Blend mode key
                    let blend_key = &bytes[offset + 4..offset + 8];
                    let blend_mode = LayerBlendMode::from_psd_key(blend_key);
                    let opacity = bytes[offset + 8];
                    let flags = bytes[offset + 10];
                    let visible = (flags & 2) == 0;
                    offset += 12;

                    if offset + 4 > bytes.len() {
                        break;
                    }
                    let extra_len = u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
                    offset += 4;

                    let mut layer_name = format!("Layer {}", layer_idx + 1);
                    if extra_len >= 8 && offset + extra_len <= bytes.len() {
                        let mask_len = u32::from_be_bytes([bytes[offset], bytes[offset + 1], bytes[offset + 2], bytes[offset + 3]]) as usize;
                        let ranges_len = u32::from_be_bytes([bytes[offset + 4], bytes[offset + 5], bytes[offset + 6], bytes[offset + 7]]) as usize;
                        let name_offset = offset + 8 + mask_len + ranges_len;
                        if name_offset < offset + extra_len {
                            let str_len = bytes[name_offset] as usize;
                            if let Some(sub) = bytes.get(name_offset + 1..name_offset + 1 + str_len)
                                && let Ok(name_str) = std::str::from_utf8(sub)
                            {
                                layer_name = name_str.to_string();
                            }
                        }
                    }
                    offset += extra_len;

                    let lw = (right.saturating_sub(left)).max(1) as usize;
                    let lh = (bottom.saturating_sub(top)).max(1) as usize;
                    let pixel_count = lw.saturating_mul(lh).min(1024 * 1024 * 64);
                    let rgba = vec![200; pixel_count.saturating_mul(4)];

                    image.add_layer(ImageLayer { name: layer_name, top, left, bottom, right, blend_mode, opacity, visible, rgba_data: rgba });
                }
            }
        }

        if image.layers.is_empty() {
            let composite_layer = ImageLayer::new("Background", width, height, image.composite_rgba.clone());
            image.add_layer(composite_layer);
        }

        Ok(image)
    }

    /// Autodetects format from raw bytes and parses into LayeredImage.
    pub fn from_bytes_auto(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.starts_with(b"\x89CRAFT\x00\x01P") {
            return Self::from_photocraft_binary(bytes);
        }
        if bytes.starts_with(b"8BPS") {
            return Self::from_psd_bytes(bytes);
        }
        // Fallback for standard flat images (PNG, JPEG, etc.) via photocraft-codecs
        if let Ok(img) = photocraft_codecs::decode_with(bytes, &photocraft_codecs::DecodeOptions::default()) {
            let (w, h) = (img.width(), img.height());
            let mut layered = Self::new("Imported Flat Image", w, h);
            let rgba = img.to_rgba8().to_vec();
            layered.composite_rgba = rgba.clone();
            layered.add_layer(ImageLayer::new("Background", w, h, rgba));
            return Ok(layered);
        }

        Err(EncoderError::InvalidContainer("unrecognized layered image container".to_string()))
    }
}

type Rgb = [f32; 3];

fn lum(c: Rgb) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

fn clip_color(c: Rgb) -> Rgb {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut out = c;
    if n < 0.0 {
        for v in &mut out {
            *v = l + (*v - l) * l / (l - n);
        }
    }
    if x > 1.0 {
        for v in &mut out {
            *v = l + (*v - l) * (1.0 - l) / (x - l);
        }
    }
    out
}

fn set_lum(c: Rgb, l: f32) -> Rgb {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

fn sat(c: Rgb) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

fn set_sat(c: Rgb, s: f32) -> Rgb {
    let mut idx = [0usize, 1, 2];
    idx.sort_by(|a, b| c[*a].partial_cmp(&c[*b]).unwrap_or(std::cmp::Ordering::Equal));
    let (min, mid, max) = (idx[0], idx[1], idx[2]);
    let mut out = [0.0f32; 3];
    if c[max] > c[min] {
        out[mid] = (c[mid] - c[min]) * s / (c[max] - c[min]);
        out[max] = s;
    }
    out
}

fn blend_channel(mode: LayerBlendMode, cb: f32, cs: f32) -> f32 {
    match mode {
        LayerBlendMode::Multiply => cb * cs,
        LayerBlendMode::Screen => cb + cs - cb * cs,
        LayerBlendMode::Overlay => blend_channel(LayerBlendMode::HardLight, cs, cb),
        LayerBlendMode::Darken => cb.min(cs),
        LayerBlendMode::Lighten => cb.max(cs),
        LayerBlendMode::ColorDodge => {
            if cb == 0.0 {
                0.0
            } else if cs >= 1.0 {
                1.0
            } else {
                (cb / (1.0 - cs)).min(1.0)
            }
        }
        LayerBlendMode::ColorBurn => {
            if cb >= 1.0 {
                1.0
            } else if cs <= 0.0 {
                0.0
            } else {
                1.0 - ((1.0 - cb) / cs).min(1.0)
            }
        }
        LayerBlendMode::HardLight => {
            if cs <= 0.5 {
                cb * 2.0 * cs
            } else {
                let s2 = 2.0 * cs - 1.0;
                cb + s2 - cb * s2
            }
        }
        LayerBlendMode::SoftLight => {
            if cs <= 0.5 {
                cb - (1.0 - 2.0 * cs) * cb * (1.0 - cb)
            } else {
                let d = if cb <= 0.25 { ((16.0 * cb - 12.0) * cb + 4.0) * cb } else { cb.sqrt() };
                cb + (2.0 * cs - 1.0) * (d - cb)
            }
        }
        LayerBlendMode::Difference => (cb - cs).abs(),
        LayerBlendMode::Exclusion => cb + cs - 2.0 * cb * cs,
        _ => cs,
    }
}

fn blend_rgb(mode: LayerBlendMode, cb: Rgb, cs: Rgb) -> Rgb {
    match mode {
        LayerBlendMode::Hue => set_lum(set_sat(cs, sat(cb)), lum(cb)),
        LayerBlendMode::Saturation => set_lum(set_sat(cb, sat(cs)), lum(cb)),
        LayerBlendMode::Color => set_lum(cs, lum(cb)),
        LayerBlendMode::Luminosity => set_lum(cb, lum(cs)),
        m => [blend_channel(m, cb[0], cs[0]), blend_channel(m, cb[1], cs[1]), blend_channel(m, cb[2], cs[2])],
    }
}

impl LayeredImage {
    /// Names of all layers, bottom to top.
    pub fn layer_names(&self) -> Vec<String> {
        self.layers.iter().map(|l| l.name.clone()).collect()
    }

    /// Keeps only the named layers visible and re-renders the flattened composite from them
    /// (W3C compositing: per-layer opacity and all sixteen blend modes) over a transparent canvas.
    /// Unknown names are an error so a typo can never silently export the wrong picture.
    pub fn select_layers(&mut self, names: &[String]) -> Result<(), EncoderError> {
        for name in names {
            if !self.layers.iter().any(|l| &l.name == name) {
                return Err(EncoderError::InvalidJobSpec(format!("layer '{name}' not found; available layers: {}", self.layer_names().join(", "))));
            }
        }
        for layer in &mut self.layers {
            layer.visible = names.contains(&layer.name);
        }
        self.composite_rgba = self.render_visible_layers();
        Ok(())
    }

    /// Flattens the currently visible layers into straight-alpha RGBA8.
    pub fn render_visible_layers(&self) -> Vec<u8> {
        let (w, h) = (self.width as usize, self.height as usize);
        let mut canvas = vec![[0.0f32; 4]; w.saturating_mul(h)];
        for layer in self.layers.iter().filter(|l| l.visible) {
            let (lw, lh) = (layer.width() as usize, layer.height() as usize);
            if layer.rgba_data.len() < lw.saturating_mul(lh).saturating_mul(4) {
                continue;
            }
            let layer_opacity = f32::from(layer.opacity) / 255.0;
            for ly in 0..lh {
                let cy = layer.top as i64 + ly as i64;
                if cy < 0 || cy >= h as i64 {
                    continue;
                }
                for lx in 0..lw {
                    let cx = layer.left as i64 + lx as i64;
                    if cx < 0 || cx >= w as i64 {
                        continue;
                    }
                    let si = (ly * lw + lx) * 4;
                    let Some(px) = layer.rgba_data.get(si..si + 4) else { continue };
                    let a_s = f32::from(px[3]) / 255.0 * layer_opacity;
                    if a_s <= 0.0 {
                        continue;
                    }
                    let cs = [f32::from(px[0]) / 255.0, f32::from(px[1]) / 255.0, f32::from(px[2]) / 255.0];
                    let Some(dst) = canvas.get_mut(cy as usize * w + cx as usize) else { continue };
                    let a_b = dst[3];
                    let cb = [dst[0], dst[1], dst[2]];
                    let blended = blend_rgb(layer.blend_mode, cb, cs);
                    let a_o = a_s + a_b * (1.0 - a_s);
                    for ch in 0..3 {
                        let mixed = (1.0 - a_b) * cs[ch] + a_b * blended[ch];
                        let premul = a_s * mixed + (1.0 - a_s) * a_b * cb[ch];
                        dst[ch] = if a_o > 0.0 { (premul / a_o).clamp(0.0, 1.0) } else { 0.0 };
                    }
                    dst[3] = a_o.clamp(0.0, 1.0);
                }
            }
        }
        let mut out = Vec::with_capacity(canvas.len() * 4);
        for p in canvas {
            out.extend(p.iter().map(|v| (v * 255.0).round() as u8));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_photocraft_binary_roundtrip() {
        let mut img = LayeredImage::new("Poster", 100, 100);
        let layer1 = ImageLayer::new("Background", 100, 100, vec![255; 40000]);
        let mut layer2 = ImageLayer::new("Text Overlay", 50, 50, vec![128; 10000]);
        layer2.blend_mode = LayerBlendMode::Multiply;
        layer2.opacity = 200;

        img.add_layer(layer1);
        img.add_layer(layer2);

        let bin = img.to_photocraft_binary().expect("serialization should succeed");
        assert!(bin.starts_with(b"\x89CRAFT\x00\x01P"));

        let roundtrip = LayeredImage::from_photocraft_binary(&bin).expect("deserialization should succeed");
        assert_eq!(roundtrip.title, "Poster");
        assert_eq!(roundtrip.layers.len(), 2);
        assert_eq!(roundtrip.layers[1].blend_mode, LayerBlendMode::Multiply);
        assert_eq!(roundtrip.layers[1].opacity, 200);
    }

    #[test]
    fn test_psd_roundtrip() {
        let mut img = LayeredImage::new("Composition", 64, 64);
        let layer1 = ImageLayer::new("Base", 64, 64, vec![100; 64 * 64 * 4]);
        img.add_layer(layer1);

        let psd_bytes = img.to_psd_bytes(false).expect("PSD export should succeed");
        assert!(psd_bytes.starts_with(b"8BPS"));
        assert_eq!(psd_bytes[4..6], [0, 1]); // Version 1

        let parsed = LayeredImage::from_psd_bytes(&psd_bytes).expect("PSD parse should succeed");
        assert_eq!(parsed.width, 64);
        assert_eq!(parsed.height, 64);
        assert_eq!(parsed.layers.len(), 1);
    }

    #[test]
    fn test_psb_export() {
        let mut img = LayeredImage::new("GiantBanner", 2000, 1000);
        img.add_layer(ImageLayer::new("Layer 1", 2000, 1000, vec![50; 1000]));

        let psb_bytes = img.to_psd_bytes(true).expect("PSB export should succeed");
        assert!(psb_bytes.starts_with(b"8BPS"));
        assert_eq!(psb_bytes[4..6], [0, 2]); // Version 2
    }

    fn solid(name: &str, w: u32, h: u32, rgba: [u8; 4]) -> ImageLayer {
        ImageLayer::new(name, w, h, rgba.iter().copied().cycle().take((w * h * 4) as usize).collect())
    }

    fn stack() -> LayeredImage {
        let mut img = LayeredImage::new("t", 2, 2);
        img.add_layer(solid("bg", 2, 2, [0, 0, 255, 255]));
        let mut half = solid("red50", 2, 2, [255, 0, 0, 255]);
        half.opacity = 128;
        img.add_layer(half);
        let mut mult = solid("mult", 2, 2, [128, 128, 128, 255]);
        mult.blend_mode = LayerBlendMode::Multiply;
        img.add_layer(mult);
        img
    }

    #[test]
    fn select_layers_recomposites_only_chosen_layers() {
        let mut img = stack();
        img.select_layers(&["bg".to_string()]).unwrap();
        assert_eq!(&img.composite_rgba[..4], &[0, 0, 255, 255]);
        assert!(img.layers.iter().find(|l| l.name == "bg").unwrap().visible);
        assert!(!img.layers.iter().find(|l| l.name == "red50").unwrap().visible);

        let mut img = stack();
        img.select_layers(&["bg".to_string(), "red50".to_string()]).unwrap();
        let px = &img.composite_rgba[..4];
        assert!((i32::from(px[0]) - 128).abs() <= 1 && (i32::from(px[2]) - 127).abs() <= 1 && px[3] == 255, "{px:?}");

        let mut img = stack();
        img.select_layers(&["bg".to_string(), "red50".to_string(), "mult".to_string()]).unwrap();
        let px = &img.composite_rgba[..4];
        assert!((i32::from(px[0]) - 64).abs() <= 1 && (i32::from(px[2]) - 64).abs() <= 1, "multiply by 50% grey halves it: {px:?}");
    }

    #[test]
    fn select_layers_rejects_unknown_names_and_handles_offsets() {
        let mut img = stack();
        let err = img.select_layers(&["nope".to_string()]).unwrap_err().to_string();
        assert!(err.contains("nope") && err.contains("bg"), "{err}");

        let mut img = LayeredImage::new("t", 4, 4);
        let mut l = solid("dot", 2, 2, [10, 20, 30, 255]);
        l.left = 3;
        l.right = 5;
        l.top = -1;
        l.bottom = 1;
        img.add_layer(l);
        img.select_layers(&["dot".to_string()]).unwrap();
        assert_eq!(&img.composite_rgba[(3) * 4..(3) * 4 + 4], &[10, 20, 30, 255]);
        assert_eq!(&img.composite_rgba[..4], &[0, 0, 0, 0]);
    }

    #[test]
    fn non_separable_and_dodge_modes_stay_in_range() {
        for mode in [
            LayerBlendMode::Hue,
            LayerBlendMode::Saturation,
            LayerBlendMode::Color,
            LayerBlendMode::Luminosity,
            LayerBlendMode::ColorDodge,
            LayerBlendMode::ColorBurn,
            LayerBlendMode::SoftLight,
            LayerBlendMode::Overlay,
        ] {
            let mut img = LayeredImage::new("t", 1, 1);
            img.add_layer(solid("a", 1, 1, [200, 40, 90, 255]));
            let mut b = solid("b", 1, 1, [30, 220, 120, 200]);
            b.blend_mode = mode;
            img.add_layer(b);
            let out = img.render_visible_layers();
            assert_eq!(out.len(), 4);
            assert_eq!(out[3], 255, "{mode:?}");
        }
    }
}
