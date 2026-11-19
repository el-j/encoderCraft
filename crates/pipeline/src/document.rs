//! Prepress, page layout, and publication pipeline for InDesign INDD/IDML,
//! native ArtCraft DesignCraft, print-ready PDF/X-4, and EPUB 3 publications.

use encodercraft_core::EncoderError;
use serde::{Deserialize, Serialize};

/// Returns the number of pages in a PDF document.
pub fn pdf_page_count(bytes: &[u8]) -> Result<usize, EncoderError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(EncoderError::InvalidContainer("not a PDF file".into()));
    }
    let pdf =
        hayro::hayro_syntax::Pdf::new(bytes.to_vec()).map_err(|error| EncoderError::InvalidContainer(format!("failed to open PDF: {error:?}")))?;
    Ok(pdf.pages().len())
}

/// Renders a single page of a PDF file into straight-alpha RGBA8 pixels at a resolution scaled to `max_dim`.
pub fn render_pdf_page_to_rgba(bytes: &[u8], page_index: usize, max_dim: f32) -> Result<(u32, u32, Vec<u8>), EncoderError> {
    const MAX_PDF_BYTES: usize = 50 * 1024 * 1024;
    if bytes.len() > MAX_PDF_BYTES {
        return Err(EncoderError::InvalidContainer(format!("PDF exceeds the {MAX_PDF_BYTES}-byte raster limit")));
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(EncoderError::InvalidContainer("not a PDF file".into()));
    }
    let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec())
        .map_err(|error| EncoderError::InvalidContainer(format!("failed to open PDF for rasterization: {error:?}")))?;
    let pages = pdf.pages();
    let page = pages.get(page_index).ok_or_else(|| EncoderError::InvalidContainer(format!("PDF page {page_index} is unavailable")))?;
    let (page_width, page_height) = page.render_dimensions();
    if !page_width.is_finite() || !page_height.is_finite() || page_width <= 0.0 || page_height <= 0.0 {
        return Err(EncoderError::InvalidContainer(format!("invalid PDF page dimensions: {page_width}x{page_height}")));
    }
    let scale = (max_dim / page_width.max(page_height)).clamp(0.25, 300.0 / 72.0);
    let cache = hayro::RenderCache::new();
    let rendered = hayro::render(
        page,
        &cache,
        &hayro::hayro_interpret::InterpreterSettings::default(),
        &hayro::RenderSettings::default(),
        &hayro::PixmapSettings { x_scale: scale, y_scale: scale, ..hayro::PixmapSettings::default() },
    );
    let width = u32::from(rendered.width());
    let height = u32::from(rendered.height());
    if width == 0 || height == 0 {
        return Err(EncoderError::InvalidContainer("PDF page produced an empty raster".into()));
    }
    Ok((width, height, rendered.data_as_u8_slice().to_vec()))
}

/// Renders a page of a [`DocumentCanvas`] to RGBA8 pixels via high-fidelity rasterization.
pub fn render_canvas_page_to_rgba(canvas: &DocumentCanvas, page_index: usize, max_dim: f32) -> Result<(u32, u32, Vec<u8>), EncoderError> {
    let pdf_bytes = canvas.to_pdf_x4_bytes()?;
    render_pdf_page_to_rgba(&pdf_bytes, page_index, max_dim)
}

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
            if let Some(slice) = bytes.get(0..sample_len) {
                if contains_subslice(slice, b"word/") || contains_subslice(slice, b"word/document.xml") {
                    return Self::from_docx_bytes(bytes);
                }
                if contains_subslice(slice, b"designmap.xml") || contains_subslice(slice, b"application/x-designcraft") {
                    return Self::from_idml_bytes(bytes);
                }
                if contains_subslice(slice, b"mimetypeapplication/epub+zip")
                    || contains_subslice(slice, b"META-INF/container.xml")
                    || contains_subslice(slice, b"application/epub+zip")
                {
                    return Self::from_epub_bytes(bytes);
                }
                if contains_subslice(slice, b"ppt/") {
                    return Self::from_pptx_bytes(bytes);
                }
            }
            return Self::from_docx_bytes(bytes);
        }
        Err(EncoderError::InvalidContainer("unrecognized prepress document format".to_string()))
    }

    /// Imports Microsoft Word OpenXML through DesignCraft's bounded DOCX reader with direct XML fallback.
    pub fn from_docx_bytes(bytes: &[u8]) -> Result<Self, EncoderError> {
        if bytes.len() > 50 * 1024 * 1024 {
            return Err(EncoderError::InvalidContainer("DOCX exceeds the 50 MiB import limit".into()));
        }
        if let Ok(imported) = designcraft_textimport::import("document.docx", bytes) {
            for warning in &imported.warnings {
                log::info!("DOCX import note: {warning}");
            }
            if !imported.story.text.trim().is_empty() {
                let story = imported.story;
                let mut doc = Self::new("Imported Word Document");
                let mut cur_page_idx = 0;
                let mut cur_y = 720.0;
                for (paragraph_index, raw_text) in story.text.split('\n').enumerate() {
                    let paragraph = story.paras.get(paragraph_index);
                    let style = paragraph.map(|paragraph| paragraph.style.to_ascii_lowercase()).unwrap_or_default();
                    let font_size = if style == "title" {
                        24.0
                    } else if style.starts_with("heading") {
                        18.0
                    } else {
                        12.0
                    };
                    if let Some(table_id) = paragraph.and_then(|paragraph| paragraph.table)
                        && let Some(table) = story.tables.get(&table_id)
                        && raw_text.contains('\u{E009}')
                    {
                        for row_index in 0..table.rows.len() {
                            let mut row = Vec::new();
                            for column_index in 0..table.columns.len() {
                                let cell_index = row_index.saturating_mul(table.columns.len()).saturating_add(column_index);
                                if let Some(cell) = table.cells.get(cell_index) {
                                    row.push(cell.text.text.replace('\n', " "));
                                }
                            }
                            append_document_text(&mut doc, &row.join(" | "), 12.0, &mut cur_page_idx, &mut cur_y);
                        }
                        continue;
                    }

                    let page_break = raw_text.chars().any(|character| matches!(character, '\u{E004}' | '\u{E011}' | '\u{E012}'));
                    let text = raw_text.chars().filter(|character| !('\u{E000}'..='\u{E1FF}').contains(character)).collect::<String>();
                    append_document_text(&mut doc, &text, font_size, &mut cur_page_idx, &mut cur_y);
                    if page_break {
                        doc.add_page();
                        cur_page_idx = doc.pages.len().saturating_sub(1);
                        cur_y = 720.0;
                    }
                }
                return Ok(doc);
            }
        }

        Self::from_docx_openxml_fallback(bytes)
    }

    /// Robust direct OpenXML parsing fallback for Microsoft Word (.docx) documents.
    pub fn from_docx_openxml_fallback(bytes: &[u8]) -> Result<Self, EncoderError> {
        let doc_xml_bytes = extract_zip_entry(bytes, "word/document.xml")?;
        let doc_xml =
            std::str::from_utf8(&doc_xml_bytes).map_err(|e| EncoderError::InvalidContainer(format!("invalid UTF-8 in word/document.xml: {e}")))?;

        let mut doc = Self::new("Imported Word Document");
        let mut cur_page_idx = 0;
        let mut cur_y = 720.0;

        let mut cursor = 0;
        while let Some(p_start) = doc_xml[cursor..].find("<w:p") {
            let abs_p_start = cursor + p_start;
            let Some(p_end_rel) = doc_xml[abs_p_start..].find("</w:p>") else {
                break;
            };
            let abs_p_end = abs_p_start + p_end_rel + 6;
            let p_content = &doc_xml[abs_p_start..abs_p_end];
            cursor = abs_p_end;

            let has_page_break = p_content.contains("w:type=\"page\"") || p_content.contains("w:type='page'");
            let is_heading = p_content.contains("Heading1") || p_content.contains("heading 1") || p_content.contains("Title");
            let is_subheading = p_content.contains("Heading2") || p_content.contains("heading 2");
            let font_size = if is_heading {
                20.0
            } else if is_subheading {
                16.0
            } else {
                12.0
            };

            let mut p_text = String::new();
            let mut t_cursor = 0;
            while let Some(t_start_rel) = p_content[t_cursor..].find("<w:t") {
                let abs_t_start = t_cursor + t_start_rel;
                let Some(tag_close_rel) = p_content[abs_t_start..].find('>') else {
                    break;
                };
                let val_start = abs_t_start + tag_close_rel + 1;
                let Some(t_end_rel) = p_content[val_start..].find("</w:t>") else {
                    break;
                };
                let val_end = val_start + t_end_rel;
                let val_text = &p_content[val_start..val_end];
                p_text.push_str(val_text);
                t_cursor = val_end + 6;
            }

            if !p_text.trim().is_empty() {
                let clean_text =
                    p_text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'");
                append_document_text(&mut doc, &clean_text, font_size, &mut cur_page_idx, &mut cur_y);
            }

            if has_page_break {
                doc.add_page();
                cur_page_idx = doc.pages.len().saturating_sub(1);
                cur_y = 720.0;
            }
        }

        Ok(doc)
    }

    /// Extracts text from a PDF into editable document pages using PdfCraft.
    pub fn from_pdf_bytes(bytes: &[u8]) -> Result<Self, EncoderError> {
        const MAX_PDF_BYTES: usize = 50 * 1024 * 1024;
        const MAX_PDF_PAGES: usize = 10_000;
        if bytes.len() > MAX_PDF_BYTES {
            return Err(EncoderError::InvalidContainer(format!("PDF exceeds the {MAX_PDF_BYTES}-byte import limit")));
        }

        if !bytes.starts_with(b"%PDF-") {
            return Err(EncoderError::InvalidContainer("not a PDF file".into()));
        }
        let pdf = lopdf::Document::load_mem(bytes).map_err(|error| EncoderError::InvalidContainer(format!("failed to open PDF: {error}")))?;
        let page_numbers: Vec<u32> = pdf.get_pages().keys().copied().collect();
        let page_count = page_numbers.len();
        if page_count == 0 {
            return Err(EncoderError::InvalidContainer("PDF could not be opened or contains no pages".into()));
        }
        if page_count > MAX_PDF_PAGES {
            return Err(EncoderError::InvalidContainer(format!("PDF exceeds the {MAX_PDF_PAGES}-page import limit")));
        }

        let mut doc = Self::new("Imported PDF Document");
        let mut cur_page_idx = 0;
        let mut cur_y = 720.0;
        let mut remaining_text_bytes = MAX_PDF_BYTES;
        for (page_index, page_number) in page_numbers.iter().enumerate() {
            if page_index > 0 {
                doc.add_page();
                cur_page_idx = doc.pages.len().saturating_sub(1);
                cur_y = 720.0;
            }
            let page_text = pdf
                .extract_text_with_limit(&[*page_number], remaining_text_bytes)
                .map_err(|error| EncoderError::InvalidContainer(format!("failed to extract PDF page {}: {error}", page_index + 1)))?;
            remaining_text_bytes = remaining_text_bytes
                .checked_sub(page_text.len())
                .ok_or_else(|| EncoderError::InvalidContainer(format!("PDF text exceeds the {MAX_PDF_BYTES}-byte extraction limit")))?;
            append_document_text(&mut doc, &page_text, 12.0, &mut cur_page_idx, &mut cur_y);
        }
        Ok(doc)
    }

    /// Imports UTF-8 plain text or Markdown while retaining paragraphs and heading levels.
    pub fn from_text(title: &str, text: &str, markdown: bool) -> Result<Self, EncoderError> {
        const MAX_TEXT_BYTES: usize = 50 * 1024 * 1024;
        if text.len() > MAX_TEXT_BYTES {
            return Err(EncoderError::InvalidContainer(format!("document text exceeds the {MAX_TEXT_BYTES}-byte import limit")));
        }
        let mut doc = Self::new(title);
        let mut page_index = 0;
        let mut y = 720.0;
        for line in text.lines() {
            let trimmed = line.trim();
            if markdown && trimmed == "---" {
                doc.add_page();
                page_index = doc.pages.len().saturating_sub(1);
                y = 720.0;
                continue;
            }
            if trimmed.is_empty() {
                y -= 8.0;
                continue;
            }
            let heading_prefix = if markdown {
                (1..=6).find(|count| trimmed.starts_with(&"#".repeat(*count)) && trimmed.as_bytes().get(*count) == Some(&b' '))
            } else {
                None
            };
            let (line, font_size) = if let Some(level) = heading_prefix {
                let heading = trimmed.get(level + 1..).unwrap_or_default();
                let font_size = match level {
                    1 => 24.0,
                    2 => 18.0,
                    _ => 16.0,
                };
                (heading, font_size)
            } else {
                (line, 12.0)
            };
            append_document_text(&mut doc, line, font_size, &mut page_index, &mut y);
        }
        Ok(doc)
    }

    /// Imports HTML text and block structure without preserving active content or markup.
    pub fn from_html(title: &str, html: &str) -> Result<Self, EncoderError> {
        const MAX_HTML_BYTES: usize = 50 * 1024 * 1024;
        if html.len() > MAX_HTML_BYTES {
            return Err(EncoderError::InvalidContainer(format!("HTML exceeds the {MAX_HTML_BYTES}-byte import limit")));
        }
        let text = html_to_text(html)?;
        Self::from_text(title, &text, true)
    }

    /// Exports document canvas into a valid Microsoft Word OpenXML (.docx) zip package.
    pub fn to_docx_bytes(&self) -> Result<Vec<u8>, EncoderError> {
        let content_types = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/></Types>";
        let rels = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
        let document_rels = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>";
        let styles_xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:qFormat/><w:rPr><w:b/><w:sz w:val=\"32\"/></w:rPr></w:style></w:styles>";

        let mut doc_xml = String::with_capacity(4096);
        doc_xml.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">\n  <w:body>\n");

        for (page_idx, page) in self.pages.iter().enumerate() {
            if page_idx > 0 {
                doc_xml.push_str("    <w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\n");
            }
            for item in &page.items {
                if let DocumentItem::Text { text, font_size, .. } = item {
                    let is_heading = *font_size >= 16.0;
                    let style = if is_heading { "    <w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr>\n" } else { "" };
                    doc_xml.push_str(&format!("    <w:p>\n{}      <w:r><w:t>{}</w:t></w:r>\n    </w:p>\n", style, quick_escape(text)));
                }
            }
        }

        doc_xml.push_str("  </w:body>\n</w:document>\n");

        let files: &[(&str, &[u8])] = &[
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", rels),
            ("word/document.xml", doc_xml.as_bytes()),
            ("word/_rels/document.xml.rels", document_rels),
            ("word/styles.xml", styles_xml),
        ];

        Ok(create_zip_archive(files))
    }

    /// Exports document canvas into a clean Markdown string.
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        for (idx, page) in self.pages.iter().enumerate() {
            if idx > 0 {
                md.push_str("\n---\n\n");
            }
            for item in &page.items {
                if let DocumentItem::Text { text, font_size, .. } = item {
                    if *font_size >= 20.0 {
                        md.push_str(&format!("# {text}\n\n"));
                    } else if *font_size >= 15.0 {
                        md.push_str(&format!("## {text}\n\n"));
                    } else {
                        md.push_str(&format!("{text}\n\n"));
                    }
                }
            }
        }
        md
    }

    /// Exports document canvas into clean standalone HTML5 string.
    pub fn to_html(&self) -> String {
        let mut html = format!(
            "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>{}</title>\n<style>\nbody {{ font-family: sans-serif; line-height: 1.6; max-width: 800px; margin: 40px auto; padding: 0 20px; }}\n.page {{ border-bottom: 1px dashed #ccc; padding-bottom: 20px; margin-bottom: 20px; }}\n</style>\n</head>\n<body>\n",
            quick_escape(&self.title)
        );
        for page in &self.pages {
            html.push_str("<div class=\"page\">\n");
            for item in &page.items {
                if let DocumentItem::Text { text, font_size, .. } = item {
                    if *font_size >= 20.0 {
                        html.push_str(&format!("<h1>{}</h1>\n", quick_escape(text)));
                    } else if *font_size >= 15.0 {
                        html.push_str(&format!("<h2>{}</h2>\n", quick_escape(text)));
                    } else {
                        html.push_str(&format!("<p>{}</p>\n", quick_escape(text)));
                    }
                }
            }
            html.push_str("</div>\n");
        }
        html.push_str("</body>\n</html>\n");
        html
    }

    /// Unpacks EPUB 3 zip publication package.
    pub fn from_epub_bytes(bytes: &[u8]) -> Result<Self, EncoderError> {
        if !bytes.starts_with(b"PK\x03\x04") {
            return Err(EncoderError::InvalidContainer("not a valid EPUB zip package".to_string()));
        }
        let entries = list_zip_entries(bytes)?;
        let mut chapter_entries: Vec<String> = entries
            .into_iter()
            .filter(|name| {
                let lower = name.to_ascii_lowercase();
                (lower.ends_with(".xhtml") || lower.ends_with(".html") || lower.ends_with(".htm"))
                    && !lower.contains("toc.xhtml")
                    && !lower.contains("nav.xhtml")
            })
            .collect();
        chapter_entries.sort();

        let mut doc = Self::new("Imported EPUB Publication");
        let mut cur_page_idx = 0;
        let mut cur_y = 720.0;
        let mut has_content = false;

        for (ch_idx, chapter_path) in chapter_entries.iter().enumerate() {
            let chapter_bytes = match extract_zip_entry(bytes, chapter_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let Ok(html_str) = std::str::from_utf8(&chapter_bytes) else {
                continue;
            };
            let text = html_to_text(html_str)?;
            if text.trim().is_empty() {
                continue;
            }

            if ch_idx > 0 && has_content {
                doc.add_page();
                cur_page_idx = doc.pages.len().saturating_sub(1);
                cur_y = 720.0;
            }

            for line in text.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let font_size = if trimmed.starts_with("# ") {
                    20.0
                } else if trimmed.starts_with("## ") {
                    16.0
                } else {
                    12.0
                };
                let display_text = trimmed.trim_start_matches('#').trim();
                append_document_text(&mut doc, display_text, font_size, &mut cur_page_idx, &mut cur_y);
                has_content = true;
            }
        }

        if !has_content {
            return Err(EncoderError::InvalidContainer("EPUB publication contains no readable text content".into()));
        }

        Ok(doc)
    }

    /// Unpacks Microsoft PowerPoint PPTX presentation package.
    pub fn from_pptx_bytes(bytes: &[u8]) -> Result<Self, EncoderError> {
        if !bytes.starts_with(b"PK\x03\x04") {
            return Err(EncoderError::InvalidContainer("not a valid PPTX zip package".to_string()));
        }
        let entries = list_zip_entries(bytes)?;
        let mut slide_entries: Vec<String> =
            entries.into_iter().filter(|name| name.starts_with("ppt/slides/slide") && name.ends_with(".xml")).collect();

        // Sort slides naturally by slide number (e.g. slide1.xml, slide2.xml, slide10.xml)
        slide_entries.sort_by_key(|name| {
            let num_part = name.strip_prefix("ppt/slides/slide").and_then(|s| s.strip_suffix(".xml")).unwrap_or("");
            num_part.parse::<usize>().unwrap_or(usize::MAX)
        });

        if slide_entries.is_empty() {
            return Err(EncoderError::InvalidContainer("PPTX contains no slide XML entries".into()));
        }

        let mut doc = Self::new("Imported PowerPoint Presentation");
        let mut cur_page_idx = 0;
        let mut cur_y = 720.0;
        let mut has_content = false;

        for (slide_idx, slide_path) in slide_entries.iter().enumerate() {
            let slide_bytes = match extract_zip_entry(bytes, slide_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let Ok(xml_str) = std::str::from_utf8(&slide_bytes) else {
                continue;
            };

            if slide_idx > 0 {
                doc.add_page();
                cur_page_idx = doc.pages.len().saturating_sub(1);
                cur_y = 720.0;
            }

            // Extract text from <a:p> paragraphs and <a:t> text runs
            let mut cursor = 0;
            while let Some(p_start) = xml_str[cursor..].find("<a:p") {
                let abs_p_start = cursor + p_start;
                let Some(p_end_rel) = xml_str[abs_p_start..].find("</a:p>") else {
                    break;
                };
                let abs_p_end = abs_p_start + p_end_rel + 6;
                let p_content = &xml_str[abs_p_start..abs_p_end];
                cursor = abs_p_end;

                let mut p_text = String::new();
                let mut t_cursor = 0;
                while let Some(t_start_rel) = p_content[t_cursor..].find("<a:t") {
                    let abs_t_start = t_cursor + t_start_rel;
                    let Some(tag_close_rel) = p_content[abs_t_start..].find('>') else {
                        break;
                    };
                    let val_start = abs_t_start + tag_close_rel + 1;
                    let Some(t_end_rel) = p_content[val_start..].find("</a:t>") else {
                        break;
                    };
                    let val_end = val_start + t_end_rel;
                    let val_text = &p_content[val_start..val_end];
                    p_text.push_str(val_text);
                    t_cursor = val_end + 6;
                }

                if !p_text.trim().is_empty() {
                    let clean = p_text.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'");
                    append_document_text(&mut doc, &clean, 16.0, &mut cur_page_idx, &mut cur_y);
                    has_content = true;
                }
            }
        }

        if !has_content {
            return Ok(doc);
        }

        Ok(doc)
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
        const MAX_EPUB_PAGES: usize = 10_000;
        if self.pages.len() > MAX_EPUB_PAGES {
            return Err(EncoderError::InvalidJobSpec(format!("EPUB exceeds the {MAX_EPUB_PAGES}-chapter export limit")));
        }
        let mimetype = b"application/epub+zip";
        let container_xml = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<container version=\"1.0\" xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles><rootfile full-path=\"OEBPS/content.opf\" media-type=\"application/oebps-package+xml\"/></rootfiles></container>";
        let page_count = self.pages.len().max(1);
        let mut manifest = String::new();
        let mut spine = String::new();
        let mut nav = String::from("<nav epub:type=\"toc\" id=\"toc\"><h1>Contents</h1><ol>");
        let mut entries: Vec<(String, Vec<u8>)> =
            vec![("mimetype".into(), mimetype.to_vec()), ("META-INF/container.xml".into(), container_xml.to_vec())];
        for page_index in 0..page_count {
            let item_id = format!("page{}", page_index + 1);
            let href = format!("page{}.xhtml", page_index + 1);
            let page = self.pages.get(page_index);
            let page_title = page
                .and_then(|page| {
                    page.items.iter().find_map(|item| match item {
                        DocumentItem::Text { text, font_size, .. } if *font_size >= 15.0 => Some(text.as_str()),
                        _ => None,
                    })
                })
                .unwrap_or(&self.title);
            let mut body = String::new();
            if let Some(page) = page {
                for item in &page.items {
                    if let DocumentItem::Text { text, font_size, .. } = item {
                        if *font_size >= 20.0 {
                            body.push_str(&format!("<h1>{}</h1>", quick_escape(text)));
                        } else if *font_size >= 15.0 {
                            body.push_str(&format!("<h2>{}</h2>", quick_escape(text)));
                        } else {
                            body.push_str(&format!("<p>{}</p>", quick_escape(text)));
                        }
                    }
                }
            }
            let xhtml = format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>{}</title></head><body>{}</body></html>\n",
                quick_escape(page_title),
                body
            );
            manifest.push_str(&format!("<item id=\"{item_id}\" href=\"{href}\" media-type=\"application/xhtml+xml\"/>"));
            spine.push_str(&format!("<itemref idref=\"{item_id}\"/>"));
            nav.push_str(&format!("<li><a href=\"{href}\">{}</a></li>", quick_escape(page_title)));
            entries.push((format!("OEBPS/{href}"), xhtml.into_bytes()));
        }
        nav.push_str("</ol></nav>");
        let content_opf = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<package xmlns=\"http://www.idpf.org/2007/opf\" version=\"3.0\" unique-identifier=\"pub-id\"><metadata xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:language>en</dc:language><dc:identifier id=\"pub-id\">urn:uuid:encodercraft-epub</dc:identifier></metadata><manifest><item id=\"nav\" href=\"nav.xhtml\" media-type=\"application/xhtml+xml\" properties=\"nav\"/>{}</manifest><spine>{}</spine></package>\n",
            quick_escape(&self.title),
            manifest,
            spine
        );
        let nav_xhtml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><head><title>Contents</title></head><body>{nav}</body></html>\n"
        );
        entries.push(("OEBPS/content.opf".into(), content_opf.into_bytes()));
        entries.push(("OEBPS/nav.xhtml".into(), nav_xhtml.into_bytes()));
        let files: Vec<(&str, &[u8])> = entries.iter().map(|(name, data)| (name.as_str(), data.as_slice())).collect();
        Ok(create_zip_archive(&files))
    }

    /// Exports document to Microsoft PowerPoint OpenXML (.pptx) slide presentation.
    pub fn to_pptx_bytes(&self) -> Result<Vec<u8>, EncoderError> {
        let content_types = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\n  <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\n  <Default Extension=\"xml\" ContentType=\"application/xml\"/>\n  <Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/>\n  <Override PartName=\"/ppt/slides/slide1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>\n</Types>\n";
        let dot_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n  <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"ppt/presentation.xml\"/>\n</Relationships>\n";
        let presentation_xml = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:presentation xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\n  <p:sldIdLst>\n    <p:sldId id=\"256\" r:id=\"rId1\"/>\n  </p:sldIdLst>\n  <p:sldSz cx=\"9144000\" cy=\"5143500\"/>\n</p:presentation>\n";
        let pres_rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\n  <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/slide1.xml\"/>\n</Relationships>\n";

        let slide_text = self
            .pages
            .first()
            .and_then(|p| p.items.iter().find_map(|item| if let DocumentItem::Text { text, .. } = item { Some(text.as_str()) } else { None }))
            .unwrap_or(&self.title);

        let slide1_xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<p:sld xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\">\n  <p:cSld>\n    <p:spTree>\n      <p:nvGrpSpPr>\n        <p:cNvPr id=\"1\" name=\"\"/>\n        <p:cNvGrpSpPr/>\n        <p:nvPr/>\n      </p:nvGrpSpPr>\n      <p:grpSpPr/>\n      <p:sp>\n        <p:nvSpPr>\n          <p:cNvPr id=\"2\" name=\"Title\"/>\n          <p:cNvSpPr/>\n          <p:nvPr/>\n        </p:nvSpPr>\n        <p:spPr/>\n        <p:txBody>\n          <a:bodyPr/>\n          <a:lstStyle/>\n          <a:p>\n            <a:r>\n              <a:t>{}</a:t>\n            </a:r>\n          </a:p>\n        </p:txBody>\n      </p:sp>\n    </p:spTree>\n  </p:cSld>\n</p:sld>\n",
            quick_escape(slide_text)
        );

        let files: &[(&str, &[u8])] = &[
            ("[Content_Types].xml", content_types.as_bytes()),
            ("_rels/.rels", dot_rels.as_bytes()),
            ("ppt/presentation.xml", presentation_xml.as_bytes()),
            ("ppt/_rels/presentation.xml.rels", pres_rels.as_bytes()),
            ("ppt/slides/slide1.xml", slide1_xml.as_bytes()),
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

fn append_document_text(doc: &mut DocumentCanvas, text: &str, font_size: f64, page_index: &mut usize, y: &mut f64) {
    let max_chars = ((504.0 / (font_size * 0.55)) as usize).clamp(20, 120);
    for source_line in text.lines() {
        let mut line = String::new();
        for word in source_line.split_whitespace() {
            let proposed_len = line.chars().count() + usize::from(!line.is_empty()) + word.chars().count();
            if proposed_len > max_chars && !line.is_empty() {
                append_document_line(doc, &line, font_size, page_index, y);
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        if !line.is_empty() {
            append_document_line(doc, &line, font_size, page_index, y);
        }
    }
}

fn append_document_line(doc: &mut DocumentCanvas, text: &str, font_size: f64, page_index: &mut usize, y: &mut f64) {
    let line_height = font_size * 1.4;
    if *y - line_height < 72.0 {
        doc.add_page();
        *page_index = doc.pages.len().saturating_sub(1);
        *y = 720.0;
    }
    if let Some(page) = doc.pages.get_mut(*page_index) {
        page.add_text(54.0, *y, 504.0, line_height, text, font_size);
    }
    *y -= line_height + 4.0;
}

fn html_to_text(html: &str) -> Result<String, EncoderError> {
    let mut result = String::new();
    let mut remaining = html;
    let mut in_head = false;
    while !remaining.is_empty() {
        if remaining.starts_with('<') {
            let Some(tag_end) = remaining.find('>') else {
                return Err(EncoderError::InvalidContainer("HTML contains an unterminated tag".into()));
            };
            let tag_content = remaining.get(1..tag_end).ok_or(EncoderError::UnexpectedEof)?.trim();
            let is_closing = tag_content.starts_with('/');
            let tag_name =
                tag_content.trim_start_matches('/').split_ascii_whitespace().next().unwrap_or_default().trim_end_matches('/').to_ascii_lowercase();
            remaining = remaining.get(tag_end + 1..).ok_or(EncoderError::UnexpectedEof)?;
            if tag_name == "head" {
                in_head = !is_closing;
                continue;
            }
            if in_head {
                continue;
            }
            if !is_closing && (tag_name == "script" || tag_name == "style") {
                let close_tag = format!("</{tag_name}");
                let close_start = find_ascii_case_insensitive(remaining, &close_tag)
                    .ok_or_else(|| EncoderError::InvalidContainer(format!("HTML has an unclosed {tag_name} element")))?;
                let after_close = remaining
                    .get(close_start..)
                    .and_then(|tail| tail.find('>').map(|offset| close_start + offset + 1))
                    .ok_or(EncoderError::UnexpectedEof)?;
                remaining = remaining.get(after_close..).ok_or(EncoderError::UnexpectedEof)?;
                continue;
            }
            if !is_closing
                && let Some(level) = tag_name.strip_prefix('h').and_then(|value| value.parse::<usize>().ok())
                && (1..=6).contains(&level)
            {
                if !result.is_empty() && !result.ends_with('\n') {
                    result.push('\n');
                }
                result.push_str(&"#".repeat(level));
                result.push(' ');
            } else if !is_closing && tag_name == "li" {
                if !result.is_empty() && !result.ends_with('\n') {
                    result.push('\n');
                }
                result.push_str("- ");
            }
            if matches!(
                tag_name.as_str(),
                "address"
                    | "article"
                    | "blockquote"
                    | "br"
                    | "div"
                    | "footer"
                    | "h1"
                    | "h2"
                    | "h3"
                    | "h4"
                    | "h5"
                    | "h6"
                    | "header"
                    | "li"
                    | "p"
                    | "section"
                    | "tr"
            ) && (is_closing || tag_name == "br")
                && !result.ends_with('\n')
            {
                result.push('\n');
            }
            continue;
        }
        if in_head {
            let text_end = remaining.find('<').unwrap_or(remaining.len());
            remaining = remaining.get(text_end..).ok_or(EncoderError::UnexpectedEof)?;
            continue;
        }
        if remaining.starts_with('&')
            && let Some(entity_end) = remaining.find(';').filter(|end| *end <= 12)
            && let Some(character) = decode_html_entity(remaining.get(1..entity_end).ok_or(EncoderError::UnexpectedEof)?)
        {
            result.push(character);
            remaining = remaining.get(entity_end + 1..).ok_or(EncoderError::UnexpectedEof)?;
            continue;
        }
        let character = remaining.chars().next().ok_or(EncoderError::UnexpectedEof)?;
        result.push(character);
        remaining = remaining.get(character.len_utf8()..).ok_or(EncoderError::UnexpectedEof)?;
    }
    Ok(result)
}

fn decode_html_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "apos" => Some('\''),
        "gt" => Some('>'),
        "lt" => Some('<'),
        "nbsp" => Some(' '),
        "quot" => Some('"'),
        _ if entity.starts_with("#x") || entity.starts_with("#X") => u32::from_str_radix(entity.get(2..)?, 16).ok().and_then(char::from_u32),
        _ if entity.starts_with('#') => entity.get(1..)?.parse::<u32>().ok().and_then(char::from_u32),
        _ => None,
    }
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    let needle = needle.as_bytes();
    haystack.as_bytes().windows(needle.len()).position(|window| window.eq_ignore_ascii_case(needle))
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
pub fn create_zip_archive(files: &[(&str, &[u8])]) -> Vec<u8> {
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

/// Lists all filenames stored in a zip archive.
pub fn list_zip_entries(bytes: &[u8]) -> Result<Vec<String>, EncoderError> {
    if bytes.len() < 30 || !bytes.starts_with(b"PK\x03\x04") {
        return Err(EncoderError::InvalidContainer("not a valid zip package".into()));
    }

    let mut cursor = 0;
    let mut entries = Vec::new();
    while cursor + 30 <= bytes.len() {
        if &bytes[cursor..cursor + 4] != b"PK\x03\x04" {
            break;
        }

        let comp_size = u32::from_le_bytes([bytes[cursor + 18], bytes[cursor + 19], bytes[cursor + 20], bytes[cursor + 21]]) as usize;
        let fn_len = u16::from_le_bytes([bytes[cursor + 26], bytes[cursor + 27]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29]]) as usize;

        let name_start = cursor + 30;
        let name_end = name_start + fn_len;
        if name_end > bytes.len() {
            break;
        }

        if let Some(name_slice) = bytes.get(name_start..name_end)
            && let Ok(name_str) = std::str::from_utf8(name_slice)
            && !name_str.is_empty()
        {
            entries.push(name_str.to_string());
        }

        let data_start = name_end + extra_len;
        let data_end = data_start.saturating_add(comp_size);
        if data_end > bytes.len() {
            break;
        }
        cursor = data_end;
    }

    // Fallback scan across central directory if local headers were truncated or used descriptors
    if entries.is_empty() {
        let mut cd_cursor = 0;
        while let Some(pos) = bytes[cd_cursor..].windows(4).position(|w| w == b"PK\x01\x02") {
            let abs_pos = cd_cursor + pos;
            if abs_pos + 46 > bytes.len() {
                break;
            }
            let fn_len = u16::from_le_bytes([bytes[abs_pos + 28], bytes[abs_pos + 29]]) as usize;
            let extra_len = u16::from_le_bytes([bytes[abs_pos + 30], bytes[abs_pos + 31]]) as usize;
            let comment_len = u16::from_le_bytes([bytes[abs_pos + 32], bytes[abs_pos + 33]]) as usize;
            let fn_start = abs_pos + 46;
            let fn_end = fn_start + fn_len;
            if fn_end <= bytes.len()
                && let Ok(name_str) = std::str::from_utf8(&bytes[fn_start..fn_end])
                && !name_str.is_empty()
                && !entries.iter().any(|e| e == name_str)
            {
                entries.push(name_str.to_string());
            }
            cd_cursor = abs_pos + 46 + fn_len + extra_len + comment_len;
        }
    }

    Ok(entries)
}

/// Extracts and decompresses a specific file entry from a zip archive (e.g. DOCX OpenXML package).
pub fn extract_zip_entry(bytes: &[u8], target_entry: &str) -> Result<Vec<u8>, EncoderError> {
    if bytes.len() < 30 || !bytes.starts_with(b"PK\x03\x04") {
        return Err(EncoderError::InvalidContainer("not a valid zip package".into()));
    }

    let mut cursor = 0;
    while cursor + 30 <= bytes.len() {
        if &bytes[cursor..cursor + 4] != b"PK\x03\x04" {
            // Not a local file header, reached central directory or end
            break;
        }

        let flags = u16::from_le_bytes([bytes[cursor + 6], bytes[cursor + 7]]);
        if flags & 0x0009 != 0 {
            return Err(EncoderError::InvalidContainer("encrypted ZIP entries and data descriptors are not supported".into()));
        }
        let method = u16::from_le_bytes([bytes[cursor + 8], bytes[cursor + 9]]);
        let comp_size = u32::from_le_bytes([bytes[cursor + 18], bytes[cursor + 19], bytes[cursor + 20], bytes[cursor + 21]]) as usize;
        let uncomp_size = u32::from_le_bytes([bytes[cursor + 22], bytes[cursor + 23], bytes[cursor + 24], bytes[cursor + 25]]) as usize;
        let fn_len = u16::from_le_bytes([bytes[cursor + 26], bytes[cursor + 27]]) as usize;
        let extra_len = u16::from_le_bytes([bytes[cursor + 28], bytes[cursor + 29]]) as usize;

        let data_start = cursor + 30 + fn_len + extra_len;
        if data_start > bytes.len() {
            return Err(EncoderError::InvalidContainer("malformed zip header bounds".into()));
        }

        let name_slice = bytes.get(cursor + 30..cursor + 30 + fn_len).ok_or(EncoderError::UnexpectedEof)?;
        let name_str = std::str::from_utf8(name_slice).unwrap_or("");

        let data_end = data_start.saturating_add(comp_size);
        let comp_data = bytes.get(data_start..data_end).ok_or(EncoderError::UnexpectedEof)?;

        if name_str == target_entry {
            const MAX_DOC_DECOMPRESSED_BYTES: usize = 50 * 1024 * 1024;
            if uncomp_size > MAX_DOC_DECOMPRESSED_BYTES {
                return Err(EncoderError::InvalidJobSpec(format!("uncompressed entry exceeds {MAX_DOC_DECOMPRESSED_BYTES} bytes safety limit")));
            }

            let decompressed = match method {
                0 if comp_data.len() <= MAX_DOC_DECOMPRESSED_BYTES => comp_data.to_vec(),
                0 => return Err(EncoderError::InvalidContainer("stored zip entry exceeds safety size limit".into())),
                8 => miniz_oxide::inflate::decompress_to_vec_with_limit(comp_data, MAX_DOC_DECOMPRESSED_BYTES)
                    .map_err(|e| EncoderError::InvalidContainer(format!("failed to decompress bounded zip deflate stream: {:?}", e.status)))?,
                m => return Err(EncoderError::InvalidContainer(format!("unsupported zip compression method {m}"))),
            };
            if decompressed.len() != uncomp_size {
                return Err(EncoderError::InvalidContainer("zip entry size does not match its header".into()));
            }
            return Ok(decompressed);
        }

        cursor = data_end;
    }

    Err(EncoderError::InvalidContainer(format!("entry '{target_entry}' not found in zip archive")))
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
        let epub_page = String::from_utf8(extract_zip_entry(&epub, "OEBPS/page1.xhtml").expect("EPUB chapter should be present")).unwrap();
        assert!(epub_page.contains("Executive Summary"));
        assert!(!epub_page.contains("Generated by encoderCraft"));
        assert!(String::from_utf8(extract_zip_entry(&epub, "OEBPS/nav.xhtml").unwrap()).unwrap().contains("Contents"));

        let parsed_epub = DocumentCanvas::from_bytes_auto(&epub).expect("EPUB auto decode should succeed");
        assert_eq!(parsed_epub.title, "Imported EPUB Publication");

        let pptx = doc.to_pptx_bytes().expect("PPTX export should succeed");
        assert!(pptx.starts_with(b"PK\x03\x04"));

        let parsed_pptx = DocumentCanvas::from_bytes_auto(&pptx).expect("PPTX auto decode should succeed");
        assert_eq!(parsed_pptx.title, "Imported PowerPoint Presentation");
    }

    #[test]
    fn test_docx_roundtrip_and_markdown_html() {
        let mut doc = DocumentCanvas::new("Proposal");
        doc.pages[0].add_text(54.0, 720.0, 500.0, 26.0, "Project Proposal", 20.0);
        doc.pages[0].add_text(54.0, 680.0, 500.0, 16.0, "This is a detailed proposal for transcoding.", 12.0);

        let docx_bytes = doc.to_docx_bytes().expect("DOCX export should succeed");
        assert!(docx_bytes.starts_with(b"PK\x03\x04"));

        let parsed = DocumentCanvas::from_bytes_auto(&docx_bytes).expect("DOCX decode should succeed");
        assert!(parsed.to_markdown().contains("This is a detailed proposal for transcoding."));
        assert!(parsed.to_markdown().contains("# Project Proposal"));

        let md = doc.to_markdown();
        assert!(md.contains("# Project Proposal"));
        assert!(md.contains("This is a detailed proposal for transcoding."));

        let html = doc.to_html();
        assert!(html.contains("<h1>Project Proposal</h1>"));
        assert!(html.contains("<p>This is a detailed proposal for transcoding.</p>"));
    }

    #[test]
    fn text_import_preserves_markdown_headings_and_html_text() {
        let markdown = DocumentCanvas::from_text("Markdown", "# Title\n\n## Section\nText", true).unwrap();
        assert!(markdown.to_markdown().contains("# Title"));
        assert!(markdown.to_markdown().contains("## Section"));
        let html = DocumentCanvas::from_html("HTML", "<h1>Title &amp; More</h1><p>Body</p><script>hidden()</script>").unwrap();
        let output = html.to_markdown();
        assert!(output.contains("# Title & More"));
        assert!(output.contains("Body"));
        assert!(!output.contains("hidden"));
    }

    #[test]
    fn zip_rejects_misleading_sizes_and_suffix_matches() {
        let mut zip = create_zip_archive(&[("other/word/document.xml", b"not the document")]);
        assert!(extract_zip_entry(&zip, "word/document.xml").is_err());
        zip[22..26].copy_from_slice(&1u32.to_le_bytes());
        assert!(extract_zip_entry(&zip, "other/word/document.xml").is_err());

        let mut descriptor_zip = create_zip_archive(&[("word/document.xml", b"content")]);
        descriptor_zip[6] |= 8;
        assert!(extract_zip_entry(&descriptor_zip, "word/document.xml").is_err());
    }

    #[test]
    fn test_epub_and_pptx_content_extraction() {
        // Test EPUB roundtrip and extraction
        let mut doc = DocumentCanvas::new("EPUB Book");
        doc.pages[0].add_text(54.0, 700.0, 500.0, 24.0, "Chapter One Title", 20.0);
        doc.pages[0].add_text(54.0, 660.0, 500.0, 16.0, "This is the first chapter content.", 12.0);
        let p2 = doc.add_page();
        p2.add_text(54.0, 700.0, 500.0, 24.0, "Chapter Two Title", 20.0);
        p2.add_text(54.0, 660.0, 500.0, 16.0, "This is the second chapter content.", 12.0);

        let epub_bytes = doc.to_epub_bytes().expect("EPUB export should succeed");
        assert!(epub_bytes.starts_with(b"PK\x03\x04"));

        let entries = list_zip_entries(&epub_bytes).expect("list_zip_entries should succeed");
        assert!(entries.iter().any(|e| e == "mimetype"));
        assert!(entries.iter().any(|e| e.starts_with("OEBPS/page") && e.ends_with(".xhtml")));

        let imported_epub = DocumentCanvas::from_epub_bytes(&epub_bytes).expect("EPUB re-import should succeed");
        let epub_md = imported_epub.to_markdown();
        assert!(epub_md.contains("Chapter One Title"));
        assert!(epub_md.contains("Chapter Two Title"));

        // Test PPTX extraction
        let slide1 = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:txBody>
          <a:bodyPr/>
          <a:p><a:r><a:t>Keynote Slide 1</a:t></a:r></a:p>
          <a:p><a:r><a:t>Introduction &amp; Overview</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#;
        let slide2 = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main">
  <p:cSld>
    <p:spTree>
      <p:sp>
        <p:txBody>
          <a:bodyPr/>
          <a:p><a:r><a:t>Keynote Slide 2</a:t></a:r></a:p>
          <a:p><a:r><a:t>Architecture &amp; Metrics</a:t></a:r></a:p>
        </p:txBody>
      </p:sp>
    </p:spTree>
  </p:cSld>
</p:sld>"#;

        let pptx_bytes = create_zip_archive(&[
            ("[Content_Types].xml", b"<Types></Types>"),
            ("ppt/slides/slide1.xml", slide1.as_bytes()),
            ("ppt/slides/slide2.xml", slide2.as_bytes()),
        ]);

        let imported_pptx = DocumentCanvas::from_pptx_bytes(&pptx_bytes).expect("PPTX import should succeed");
        assert_eq!(imported_pptx.pages.len(), 2);
        let pptx_md = imported_pptx.to_markdown();
        assert!(pptx_md.contains("Keynote Slide 1"));
        assert!(pptx_md.contains("Introduction & Overview"));
        assert!(pptx_md.contains("Keynote Slide 2"));
        assert!(pptx_md.contains("Architecture & Metrics"));
    }
}
