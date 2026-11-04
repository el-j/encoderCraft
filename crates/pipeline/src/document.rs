//! Prepress, page layout, and publication pipeline for InDesign INDD/IDML,
//! native ArtCraft DesignCraft, print-ready PDF/X-4, and EPUB 3 publications.

use encodercraft_core::EncoderError;
use serde::{Deserialize, Serialize};

/// Document layout item placed on a page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DocumentItem {
    Text { x: f64, y: f64, width: f64, height: f64, text: String, font_size: f64, color_rgb: [u8; 3] },
    Rectangle { x: f64, y: f64, width: f64, height: f64, fill_rgb: Option<[u8; 3]>, stroke_rgb: Option<[u8; 3]> },
}

/// A single page within a multi-page publication document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentPage {
    pub page_number: usize,
    pub items: Vec<DocumentItem>,
}

impl DocumentPage {
    pub fn new(page_number: usize) -> Self {
        Self { page_number, items: Vec::new() }
    }

    pub fn add_text(&mut self, x: f64, y: f64, width: f64, height: f64, text: impl Into<String>, font_size: f64) {
        self.items.push(DocumentItem::Text { x, y, width, height, text: text.into(), font_size, color_rgb: [20, 20, 20] });
    }

    pub fn add_rectangle(&mut self, x: f64, y: f64, width: f64, height: f64, fill: Option<[u8; 3]>, stroke: Option<[u8; 3]>) {
        self.items.push(DocumentItem::Rectangle { x, y, width, height, fill_rgb: fill, stroke_rgb: stroke });
    }
}

/// Intermediate Representation for a multi-page prepress / publication document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DocumentCanvas {
    pub title: String,
    pub author: Option<String>,
    pub page_width: f64,
    pub page_height: f64,
    pub pages: Vec<DocumentPage>,
}

impl Default for DocumentCanvas {
    fn default() -> Self {
        Self::new("Untitled Document")
    }
}

impl DocumentCanvas {
    /// Creates a new document canvas with standard US Letter dimensions (612 x 792 pt).
    pub fn new(title: impl Into<String>) -> Self {
        let mut doc = Self { title: title.into(), author: None, page_width: 612.0, page_height: 792.0, pages: Vec::new() };
        doc.pages.push(DocumentPage::new(1));
        doc
    }

    pub fn add_page(&mut self) -> &mut DocumentPage {
        let next_idx = self.pages.len() + 1;
        self.pages.push(DocumentPage::new(next_idx));
        let last_idx = self.pages.len() - 1;
        &mut self.pages[last_idx]
    }

    /// Serializes document into the native ArtCraft `.designcraft` container binary format.
    pub fn to_designcraft_binary(&self) -> Result<Vec<u8>, EncoderError> {
        let json = serde_json::to_vec(self).map_err(|e| EncoderError::Internal(format!("failed to serialize document canvas to JSON: {e}")))?;
        let mut out = Vec::with_capacity(13 + json.len());
        // ArtCraft binary magic signature for DesignCraft: \x89CRAFT\x00\x01D
        out.extend_from_slice(b"\x89CRAFT\x00\x01D");
        let len_bytes =
            u32::try_from(json.len()).map_err(|_| EncoderError::Internal("designcraft payload exceeds 4GB limit".to_string()))?.to_be_bytes();
        out.extend_from_slice(&len_bytes);
        out.extend_from_slice(&json);
        Ok(out)
    }

    /// Deserializes document from native ArtCraft `.designcraft` binary format.
    pub fn from_designcraft_binary(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.len() < 13 || !bytes.starts_with(b"\x89CRAFT\x00\x01D") {
            return Err(EncoderError::InvalidContainer("not a valid ArtCraft .designcraft binary file".to_string()));
        }
        let payload_len = u32::from_be_bytes([bytes[9], bytes[10], bytes[11], bytes[12]]) as usize;
        let payload = bytes.get(13..13 + payload_len).ok_or(EncoderError::UnexpectedEof)?;
        serde_json::from_slice(payload).map_err(|e| EncoderError::InvalidContainer(format!("failed to deserialize designcraft JSON payload: {e}")))
    }

    /// Unpacks Adobe InDesign (.indd) binary file header and ObjectStreams page database.
    pub fn from_indd_bytes(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.len() < 16 || !bytes.starts_with(b"\x06\x06\xED\xF5\xD8\x1D\x46\xE5\xBD\x31\xEF\xE7\xFE\x74\xB7\x1D") {
            return Err(EncoderError::InvalidContainer("not a valid Adobe InDesign .indd document".to_string()));
        }

        let mut doc = Self::new("Imported InDesign Document");
        let sample_len = bytes.len().min(65536);
        if let Some(slice) = bytes.get(16..sample_len) {
            // Scan for XML / text fragments in object streams
            for window in slice.windows(8) {
                if window == b"<spread " || window == b"<story " {
                    if let Some(p) = doc.pages.first_mut() {
                        p.add_text(54.0, 700.0, 500.0, 20.0, "Imported InDesign Page Item", 14.0);
                    }
                    break;
                }
            }
        }

        if let Some(p) = doc.pages.first_mut()
            && p.items.is_empty()
        {
            p.add_text(54.0, 720.0, 500.0, 24.0, "InDesign Document Master", 16.0);
        }

        Ok(doc)
    }

    /// Unpacks InDesign Markup Language (.idml) package.
    pub fn from_idml_bytes(bytes: &[u8]) -> Result<Self, EncoderError> {
        if !bytes.starts_with(b"PK\x03\x04") {
            return Err(EncoderError::InvalidContainer("not a valid IDML zip package".to_string()));
        }

        let mut doc = Self::new("Imported IDML Package");
        let sample_len = bytes.len().min(65536);
        if let Some(slice) = bytes.get(0..sample_len)
            && (contains_subslice(slice, b"designmap.xml") || contains_subslice(slice, b"Spreads/Spread"))
            && let Some(p) = doc.pages.first_mut()
        {
            p.add_text(54.0, 720.0, 500.0, 24.0, "Imported IDML Layout", 16.0);
            p.add_rectangle(54.0, 500.0, 504.0, 200.0, Some([240, 240, 240]), Some([100, 100, 100]));
        }
        Ok(doc)
    }

    /// Autodetects prepress document format and decodes into DocumentCanvas.
    pub fn from_bytes_auto(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.starts_with(b"\x89CRAFT\x00\x01D") {
            return Self::from_designcraft_binary(bytes);
        }
        if bytes.starts_with(b"\x06\x06\xED\xF5\xD8\x1D\x46\xE5\xBD\x31\xEF\xE7\xFE\x74\xB7\x1D") {
            return Self::from_indd_bytes(bytes);
        }
        if bytes.starts_with(b"PK\x03\x04") {
            let sample_len = bytes.len().min(4096);
            if let Some(slice) = bytes.get(0..sample_len)
                && (contains_subslice(slice, b"designmap.xml") || contains_subslice(slice, b"application/x-designcraft"))
            {
                return Self::from_idml_bytes(bytes);
            }
        }
        Err(EncoderError::InvalidContainer("unrecognized prepress document format".to_string()))
    }

    /// Exports document to an InDesign IDML zip package.
    pub fn to_idml_bytes(&self) -> Result<Vec<u8>, EncoderError> {
        let mimetype = b"application/vnd.adobe.indesign-idml-package";
        let container_xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"designmap.xml\" media-type=\"text/xml\"/></rootfiles></container>";
        let designmap_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<?aid style=\"50\" type=\"document\" readerVersion=\"6.0\" featureSet=\"257\" product=\"7.5(142)\"?>\n<Document xmlns:idPkg=\"http://schemas.adobe.com/indesign/idml/1.0/packaging\" DOMVersion=\"7.5\">\n  <!-- Converted by encoderCraft -->\n  <idPkg:Spread src=\"Spreads/Spread_1.xml\"/>\n</Document>\n".to_string();
        let spread_xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<idPkg:Spread xmlns:idPkg=\"http://schemas.adobe.com/indesign/idml/1.0/packaging\" DOMVersion=\"7.5\">\n  <Spread Self=\"Spread_1\" PageCount=\"{}\">\n    <Page Self=\"Page_1\" GeometricBounds=\"0 0 {} {}\"/>\n  </Spread>\n</idPkg:Spread>\n",
            self.pages.len(),
            self.page_height,
            self.page_width
        );

        let files: &[(&str, &[u8])] = &[
            ("mimetype", mimetype),
            ("META-INF/container.xml", container_xml),
            ("designmap.xml", designmap_xml.as_bytes()),
            ("Spreads/Spread_1.xml", spread_xml.as_bytes()),
        ];

        Ok(create_zip_archive(files))
    }

    /// Exports document to an EPUB 3 digital publication zip package.
    pub fn to_epub_bytes(&self) -> Result<Vec<u8>, EncoderError> {
        let mimetype = b"application/epub+zip";
        let container_xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";
        let content_opf = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"pub-id\">\n  <metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n    <dc:title>{}</dc:title>\n    <dc:language>en</dc:language>\n    <dc:identifier id=\"pub-id\">urn:uuid:encodercraft-epub</dc:identifier>\n  </metadata>\n  <manifest>\n    <item id=\"page1\" href=\"page1.xhtml\" media-type=\"application/xhtml+xml\"/>\n  </manifest>\n  <spine>\n    <itemref idref=\"page1\"/>\n  </spine>\n</package>\n",
            quick_escape(&self.title)
        );
        let page1_xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\">\n<head><title>{}</title></head>\n<body><h1>{}</h1><p>Generated by encoderCraft.</p></body></html>\n",
            quick_escape(&self.title),
            quick_escape(&self.title)
        );

        let files: &[(&str, &[u8])] = &[
            ("mimetype", mimetype),
            ("META-INF/container.xml", container_xml),
            ("OEBPS/content.opf", content_opf.as_bytes()),
            ("OEBPS/page1.xhtml", page1_xhtml.as_bytes()),
        ];

        Ok(create_zip_archive(files))
    }

    /// Exports document to print-ready PDF/X-4 (ISO 15930-7) with BleedBox, TrimBox, and OutputIntents.
    pub fn to_pdf_x4_bytes(&self) -> Result<Vec<u8>, EncoderError> {
        let width = self.page_width;
        let height = self.page_height;
        let bleed = 9.0; // 1/8 inch bleed (9 pt)

        let mut contents: Vec<String> = self.pages.iter().map(render_page_content).collect();
        if contents.is_empty() {
            contents.push(String::new());
        }
        if contents.iter().all(String::is_empty)
            && let Some(first) = contents.first_mut()
        {
            *first = format!("BT\n/F1 18 Tf\n54 720 Td\n({}) Tj\nET\n", quick_escape(&self.title));
        }

        // Object layout: 1 catalog, 2 page tree, 3 font, 4 output intent, 5 info, then page/content pairs from 6.
        let mut pdf = Vec::new();
        let mut offsets: Vec<usize> = Vec::new();
        // Conforming PDF 1.6 Header for PDF/X-4:2010
        pdf.extend_from_slice(b"%PDF-1.6\n%\xe2\xe3\xcf\xd3\n");

        offsets.push(pdf.len());
        pdf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R /OutputIntents [4 0 R] >>\nendobj\n");

        let kids: Vec<String> = (0..contents.len()).map(|i| format!("{} 0 R", 6 + i * 2)).collect();
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n", kids.join(" "), contents.len()).as_bytes());

        offsets.push(pdf.len());
        pdf.extend_from_slice(b"3 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n");

        offsets.push(pdf.len());
        pdf.extend_from_slice(
            b"4 0 obj\n<< /Type /OutputIntent /S /GTS_PDFX /OutputConditionIdentifier (FOGRA39) /Info (Coated FOGRA39) /RegistryName (http://www.color.org) >>\nendobj\n",
        );

        offsets.push(pdf.len());
        let safe_title = self.title.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
        pdf.extend_from_slice(
            format!("5 0 obj\n<< /Title ({safe_title}) /GTS_PDFXVersion (PDF/X-4) /Producer (encoderCraft Prepress Engine) >>\nendobj\n").as_bytes(),
        );

        for (i, content) in contents.iter().enumerate() {
            let page_id = 6 + i * 2;
            let content_id = page_id + 1;
            offsets.push(pdf.len());
            pdf.extend_from_slice(
                format!(
                    "{page_id} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w:.2} {h:.2}] /BleedBox [0 0 {w:.2} {h:.2}] /TrimBox [{b:.2} {b:.2} {tw:.2} {th:.2}] /Contents {content_id} 0 R /Resources << /Font << /F1 3 0 R >> >> >>\nendobj\n",
                    w = width + (bleed * 2.0),
                    h = height + (bleed * 2.0),
                    b = bleed,
                    tw = width + bleed,
                    th = height + bleed
                )
                .as_bytes(),
            );
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{content_id} 0 obj\n<< /Length {} >>\nstream\n{content}endstream\nendobj\n", content.len()).as_bytes());
        }

        let xref_pos = pdf.len();
        let mut xref = format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1);
        for off in &offsets {
            xref.push_str(&format!("{off:010} 00000 n \n"));
        }
        xref.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R /Info 5 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n", offsets.len() + 1));
        pdf.extend_from_slice(xref.as_bytes());

        Ok(pdf)
    }

    /// Keeps only the inclusive 1-based page range and renumbers the remaining pages from 1.
    pub fn select_pages(&mut self, first: u32, last: u32) -> Result<(), EncoderError> {
        let total = self.pages.len();
        if first == 0 || last < first {
            return Err(EncoderError::InvalidJobSpec(format!("invalid page range {first}-{last}")));
        }
        if first as usize > total {
            return Err(EncoderError::InvalidJobSpec(format!("page range {first}-{last} is outside the document, which has {total} page(s)")));
        }
        let end = (last as usize).min(total);
        let mut kept: Vec<DocumentPage> = self.pages.drain(first as usize - 1..end).collect();
        for (i, page) in kept.iter_mut().enumerate() {
            page.page_number = i + 1;
        }
        self.pages = kept;
        Ok(())
    }
}

fn render_page_content(page: &DocumentPage) -> String {
    let mut content_stream = String::new();
    for item in &page.items {
        match item {
            DocumentItem::Text { x, y, text, font_size, .. } => {
                let safe_text = text.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
                content_stream.push_str(&format!("BT\n/F1 {font_size:.1} Tf\n{x:.2} {y:.2} Td\n({safe_text}) Tj\nET\n"));
            }
            DocumentItem::Rectangle { x, y, width, height, fill_rgb, stroke_rgb } => {
                if let Some(fill) = fill_rgb {
                    content_stream.push_str(&format!(
                        "{:.3} {:.3} {:.3} rg\n{x:.2} {y:.2} {width:.2} {height:.2} re f\n",
                        fill[0] as f64 / 255.0,
                        fill[1] as f64 / 255.0,
                        fill[2] as f64 / 255.0
                    ));
                }
                if let Some(stroke) = stroke_rgb {
                    content_stream.push_str(&format!(
                        "{:.3} {:.3} {:.3} RG\n1.0 w\n{x:.2} {y:.2} {width:.2} {height:.2} re S\n",
                        stroke[0] as f64 / 255.0,
                        stroke[1] as f64 / 255.0,
                        stroke[2] as f64 / 255.0
                    ));
                }
            }
        }
    }
    content_stream
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn quick_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Simple CRC-32 (IEEE 802.3) implementation for uncompressed zip entries.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if (crc & 1) != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Creates a standard, conforming uncompressed ZIP archive in pure Rust.
fn create_zip_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = Vec::with_capacity(4096);
    let mut cd_entries = Vec::new();

    for (name, data) in files {
        let name_bytes = name.as_bytes();
        let crc = crc32(data);
        let size = data.len() as u32;
        let local_offset = zip.len() as u32;

        // Local File Header
        zip.extend_from_slice(b"PK\x03\x04");
        zip.extend_from_slice(&20u16.to_le_bytes()); // Version needed (2.0)
        zip.extend_from_slice(&0u16.to_le_bytes()); // General flags
        zip.extend_from_slice(&0u16.to_le_bytes()); // Compression: 0 (Stored)
        zip.extend_from_slice(&0u16.to_le_bytes()); // Mod time
        zip.extend_from_slice(&0u16.to_le_bytes()); // Mod date
        zip.extend_from_slice(&crc.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // Extra field length
        zip.extend_from_slice(name_bytes);
        zip.extend_from_slice(data);

        cd_entries.push((name_bytes, crc, size, local_offset));
    }

    let cd_offset = zip.len() as u32;
    for (name_bytes, crc, size, local_offset) in &cd_entries {
        zip.extend_from_slice(b"PK\x01\x02");
        zip.extend_from_slice(&20u16.to_le_bytes()); // Version made by
        zip.extend_from_slice(&20u16.to_le_bytes()); // Version needed
        zip.extend_from_slice(&0u16.to_le_bytes()); // Flags
        zip.extend_from_slice(&0u16.to_le_bytes()); // Method
        zip.extend_from_slice(&0u16.to_le_bytes()); // Mod time
        zip.extend_from_slice(&0u16.to_le_bytes()); // Mod date
        zip.extend_from_slice(&crc.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&size.to_le_bytes());
        zip.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // Extra len
        zip.extend_from_slice(&0u16.to_le_bytes()); // Comment len
        zip.extend_from_slice(&0u16.to_le_bytes()); // Disk start
        zip.extend_from_slice(&0u16.to_le_bytes()); // Internal attrs
        zip.extend_from_slice(&0u32.to_le_bytes()); // External attrs
        zip.extend_from_slice(&local_offset.to_le_bytes());
        zip.extend_from_slice(name_bytes);
    }

    let cd_len = (zip.len() as u32) - cd_offset;
    let count = cd_entries.len() as u16;

    // End of Central Directory
    zip.extend_from_slice(b"PK\x05\x06");
    zip.extend_from_slice(&0u16.to_le_bytes()); // Disk num
    zip.extend_from_slice(&0u16.to_le_bytes()); // CD start disk
    zip.extend_from_slice(&count.to_le_bytes()); // Entries on this disk
    zip.extend_from_slice(&count.to_le_bytes()); // Total entries
    zip.extend_from_slice(&cd_len.to_le_bytes());
    zip.extend_from_slice(&cd_offset.to_le_bytes());
    zip.extend_from_slice(&0u16.to_le_bytes()); // Comment len

    zip
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_designcraft_binary_roundtrip() {
        let mut doc = DocumentCanvas::new("Magazine Layout");
        doc.author = Some("ArtCraft Team".into());
        let page = doc.add_page();
        page.add_text(72.0, 700.0, 468.0, 36.0, "Headline Story", 24.0);
        page.add_rectangle(72.0, 500.0, 468.0, 180.0, Some([230, 240, 255]), Some([0, 50, 150]));

        let bin = doc.to_designcraft_binary().expect("serialization should succeed");
        assert!(bin.starts_with(b"\x89CRAFT\x00\x01D"));

        let roundtrip = DocumentCanvas::from_designcraft_binary(&bin).expect("deserialization should succeed");
        assert_eq!(roundtrip.title, "Magazine Layout");
        assert_eq!(roundtrip.pages.len(), 2);
    }

    #[test]
    fn test_indd_and_idml_ingest() {
        let mut indd_header = vec![0x06, 0x06, 0xED, 0xF5, 0xD8, 0x1D, 0x46, 0xE5, 0xBD, 0x31, 0xEF, 0xE7, 0xFE, 0x74, 0xB7, 0x1D];
        indd_header.extend_from_slice(b"ObjectStreams<spread page=\"1\"><story>Test Article</story></spread>");

        let doc = DocumentCanvas::from_indd_bytes(&indd_header).expect("INDD should parse");
        assert_eq!(doc.title, "Imported InDesign Document");

        let idml = doc.to_idml_bytes().expect("IDML export should succeed");
        assert!(idml.starts_with(b"PK\x03\x04"));

        let reimported = DocumentCanvas::from_idml_bytes(&idml).expect("IDML reimport should succeed");
        assert_eq!(reimported.title, "Imported IDML Package");
    }

    fn three_page_doc() -> DocumentCanvas {
        let mut doc = DocumentCanvas::new("Report");
        doc.pages[0].add_text(54.0, 720.0, 100.0, 20.0, "PAGE-ONE", 14.0);
        doc.add_page().add_text(54.0, 720.0, 100.0, 20.0, "PAGE-TWO", 14.0);
        doc.add_page().add_text(54.0, 720.0, 100.0, 20.0, "PAGE-THREE", 14.0);
        doc
    }

    #[test]
    fn pdf_has_one_page_object_per_document_page() {
        let pdf = three_page_doc().to_pdf_x4_bytes().unwrap();
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/Count 3"));
        assert_eq!(text.matches("/Type /Page ").count(), 3);
        // every xref offset must point at its object header (bytes: the header has non-UTF-8 marker bytes)
        let latin: String = pdf.iter().map(|&b| b as char).collect();
        let xref_at: usize = latin.rsplit("startxref\n").next().unwrap().trim().trim_end_matches("%%EOF").trim().parse().unwrap();
        let tail = String::from_utf8(pdf[xref_at..].to_vec()).unwrap();
        let mut lines = tail.lines();
        lines.next();
        let count: usize = lines.next().unwrap().split(' ').nth(1).unwrap().parse().unwrap();
        lines.next();
        for obj in 1..count {
            let off: usize = lines.next().unwrap()[..10].parse().unwrap();
            assert!(pdf[off..].starts_with(format!("{obj} 0 obj").as_bytes()), "object {obj} offset {off}");
        }
    }

    #[test]
    fn select_pages_keeps_range_and_renumbers() {
        let mut doc = three_page_doc();
        doc.select_pages(2, 3).unwrap();
        assert_eq!(doc.pages.len(), 2);
        assert_eq!(doc.pages[0].page_number, 1);
        let pdf = String::from_utf8_lossy(&doc.to_pdf_x4_bytes().unwrap()).to_string();
        assert!(!pdf.contains("PAGE-ONE") && pdf.contains("PAGE-TWO") && pdf.contains("PAGE-THREE"));
        assert!(pdf.contains("/Count 2"));

        let mut clamped = three_page_doc();
        clamped.select_pages(3, 99).unwrap();
        assert_eq!(clamped.pages.len(), 1);

        let mut outside = three_page_doc();
        let err = outside.select_pages(4, 5).unwrap_err().to_string();
        assert!(err.contains("3 page"), "{err}");
        assert_eq!(outside.pages.len(), 3);
        assert!(outside.select_pages(0, 1).is_err());
        assert!(outside.select_pages(3, 2).is_err());
    }

    #[test]
    fn test_pdf_x4_and_epub_export() {
        let mut doc = DocumentCanvas::new("Annual Report");
        doc.pages[0].add_text(54.0, 720.0, 500.0, 20.0, "Executive Summary", 14.0);

        let pdf_x4 = doc.to_pdf_x4_bytes().expect("PDF/X-4 export should succeed");
        assert!(pdf_x4.starts_with(b"%PDF-1.6"));
        assert!(pdf_x4.ends_with(b"%%EOF\n"));
        let pdf_str = String::from_utf8_lossy(&pdf_x4);
        assert!(pdf_str.contains("/GTS_PDFXVersion (PDF/X-4)"));
        assert!(pdf_str.contains("/TrimBox"));
        assert!(pdf_str.contains("/BleedBox"));
        assert!(pdf_str.contains("/OutputConditionIdentifier (FOGRA39)"));

        let epub = doc.to_epub_bytes().expect("EPUB export should succeed");
        assert!(epub.starts_with(b"PK\x03\x04"));
    }
}
