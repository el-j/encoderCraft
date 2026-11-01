//! Output path template expansion and file naming token engine.
//!
//! Replaces Adobe Media Encoder's naming tokens with a flexible template syntax:
//! `{source}_{preset}_{width}x{height}_{date}.{ext}`.
//! Supports collision avoidance (auto-incrementing suffixes) and sanitization.

use std::path::{Path, PathBuf};

use chrono::Local;

/// Collision resolution policy when the destination output path already exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CollisionPolicy {
    /// Overwrites any existing file at the target path.
    Overwrite,
    /// Appends an incremental counter suffix `_01`, `_02` to avoid clobbering existing media.
    #[default]
    AutoIncrement,
    /// Fails immediately if the target file already exists.
    FailIfExists,
}

/// Token context variables available for template substitution.
#[derive(Debug, Clone)]
pub struct NamingContext<'a> {
    pub source_path: &'a Path,
    pub preset_name: &'a str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub extension: &'a str,
}

/// Expands a template string using properties from `NamingContext`.
pub fn expand_template(template: &str, ctx: &NamingContext) -> String {
    let source_name = ctx.source_path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("source");

    let source_stem = ctx.source_path.file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("source");

    let preset_slug = ctx.preset_name.to_ascii_lowercase().replace([' ', '-', '/'], "_");

    let now = Local::now();
    let date_str = now.format("%Y-%m-%d").to_string();
    let time_str = now.format("%H%M%S").to_string();

    let width_str = ctx.width.map(|w| w.to_string()).unwrap_or_else(|| "orig".into());
    let height_str = ctx.height.map(|h| h.to_string()).unwrap_or_else(|| "orig".into());
    let resolution_str = format!("{width_str}x{height_str}");

    let fps_str = ctx.frame_rate.map(|f| format!("{f:.2}").trim_end_matches(".00").to_string()).unwrap_or_else(|| "orig".into());

    let ext_str = ctx.extension.trim_start_matches('.');

    let mut result = template.to_string();
    result = result.replace("{source}", source_stem);
    result = result.replace("{source_name}", source_name);
    result = result.replace("{source_stem}", source_stem);
    result = result.replace("{preset}", ctx.preset_name);
    result = result.replace("{preset_slug}", &preset_slug);
    result = result.replace("{width}", &width_str);
    result = result.replace("{height}", &height_str);
    result = result.replace("{resolution}", &resolution_str);
    result = result.replace("{fps}", &fps_str);
    result = result.replace("{date}", &date_str);
    result = result.replace("{time}", &time_str);
    result = result.replace("{ext}", ext_str);

    result
}

/// Resolves a final output path given a directory, template, context, and collision policy.
pub fn resolve_output_path(output_dir: &Path, template: &str, ctx: &NamingContext, policy: CollisionPolicy) -> Result<PathBuf, String> {
    let filename = expand_template(template, ctx);
    let candidate = output_dir.join(&filename);

    if !candidate.exists() || policy == CollisionPolicy::Overwrite {
        return Ok(candidate);
    }

    if policy == CollisionPolicy::FailIfExists {
        return Err(format!("destination file `{}` already exists", candidate.display()));
    }

    // AutoIncrement: candidate_01.ext, candidate_02.ext
    let stem = candidate.file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or("output");
    let ext = candidate.extension().and_then(std::ffi::OsStr::to_str).unwrap_or(ctx.extension);

    for i in 1..=9999 {
        let suffixed_name = format!("{stem}_{i:02}.{ext}");
        let test_path = output_dir.join(suffixed_name);
        if !test_path.exists() {
            return Ok(test_path);
        }
    }

    Err("exhausted auto-increment limit (9999 files with identical name exist)".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_expand_template() {
        let src = Path::new("/media/raw/A001_C001_1028_001.mov");
        let ctx = NamingContext {
            source_path: src,
            preset_name: "YouTube 4K",
            width: Some(3840),
            height: Some(2160),
            frame_rate: Some(24.0),
            extension: "mp4",
        };

        let template = "{source}_{preset_slug}_{resolution}.{ext}";
        let res = expand_template(template, &ctx);
        assert_eq!(res, "A001_C001_1028_001_youtube_4k_3840x2160.mp4");
    }

    #[test]
    fn test_collision_autoincrement() {
        let dir = tempdir().unwrap();
        let src = Path::new("clip.mp4");
        let ctx = NamingContext {
            source_path: src,
            preset_name: "ProRes",
            width: Some(1920),
            height: Some(1080),
            frame_rate: Some(30.0),
            extension: "mov",
        };

        let template = "{source}.{ext}";

        // 1st candidate: clip.mov
        let p1 = resolve_output_path(dir.path(), template, &ctx, CollisionPolicy::AutoIncrement).unwrap();
        assert_eq!(p1, dir.path().join("clip.mov"));
        std::fs::write(&p1, b"test").unwrap();

        // 2nd candidate: clip_01.mov
        let p2 = resolve_output_path(dir.path(), template, &ctx, CollisionPolicy::AutoIncrement).unwrap();
        assert_eq!(p2, dir.path().join("clip_01.mov"));
        std::fs::write(&p2, b"test").unwrap();

        // 3rd candidate: clip_02.mov
        let p3 = resolve_output_path(dir.path(), template, &ctx, CollisionPolicy::AutoIncrement).unwrap();
        assert_eq!(p3, dir.path().join("clip_02.mov"));
    }
}
