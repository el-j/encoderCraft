use std::fmt::Write as _;
use std::path::{Path, PathBuf};

const WEB_FONTS: &[(&str, &str)] = &[
    ("BIZ UDPGothic", "Regular"),
    ("BIZ UDPGothic", "Bold"),
    ("NanumGothic", "Regular"),
    ("Noto Sans Symbols", "Regular"),
    ("Noto Sans Symbols 2", "Regular"),
];

fn main() {
    // craft-fonts faces from `CRAFT_FONTS_DIR`, embedded as every Craft app embeds them (craft-fonts-build): unset, the
    // table is empty; `CRAFT_FONTS_REQUIRED=1` makes a missing or broken checkout an error. A web build keeps only the
    // faces its size budget allows.
    let wasm = craft_fonts_build::target_is_wasm();
    let required = std::env::var("CRAFT_FONTS_REQUIRED").is_ok_and(|value| value.trim() == "1");
    match craft_fonts_build::Embed::new()
        .filter(move |face| !wasm || WEB_FONTS.contains(&(face.family.as_str(), face.style.as_str())))
        .write("craft_fonts.rs")
    {
        Ok(embedded) if wasm && required => {
            for &(family, style) in WEB_FONTS {
                if !embedded.iter().any(|face| face.family == family && face.style == style) {
                    println!("cargo::error=required web fallback face is missing: {family} {style}");
                }
            }
        }
        Ok(_) => {}
        Err(error) => println!("cargo::error=craft-fonts: {error}"),
    }

    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        println!("cargo::error=OUT_DIR is missing");
        return;
    };
    let output = out_dir.join("craft_i18n.rs");
    if let Err(error) = std::fs::write(&output, craft_i18n()) {
        println!("cargo::error=writing {}: {error}", output.display());
    }
}

/// The catalog encoderCraft reads in each locale: `locales/<locale>/encodercraft.tsv`.
const APP_CATALOG: &str = "encodercraft.tsv";

/// `CRAFT_I18N` from `CRAFT_I18N_DIR`, a storytold/craft-i18n checkout: one (locale, shared catalog, encoderCraft
/// catalog) entry per locale that has an `encodercraft.tsv`. Unset, or not a checkout (a warning), it is empty and the
/// interface is English only; with `CRAFT_I18N_REQUIRED=1` (releases) that is an error instead. The recipe is
/// craft-i18n's `docs/integration.md`; this follows pdfCraft's build script (MIT OR Apache-2.0).
fn craft_i18n() -> String {
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_I18N_REQUIRED");
    let required = std::env::var("CRAFT_I18N_REQUIRED").is_ok_and(|value| value.trim() == "1");
    let entries = match std::env::var_os("CRAFT_I18N_DIR").filter(|dir| !dir.is_empty()) {
        // Not canonicalized: on Windows that gives `\\?\C:\…`, which `include_str!` can't read.
        Some(dir) => match locales(&PathBuf::from(dir).join("locales")) {
            Ok(entries) => entries,
            Err(error) => {
                let level = if required { "error" } else { "warning" };
                println!("cargo::{level}=CRAFT_I18N_DIR: {error}; the interface is English only");
                Vec::new()
            }
        },
        None => {
            if required {
                println!("cargo::error=CRAFT_I18N_REQUIRED=1 but CRAFT_I18N_DIR is unset: a release must embed the translations");
            }
            Vec::new()
        }
    };
    let mut source = String::from("/// (locale, shared catalog, encoderCraft's catalog), one entry per locale; `\"\"` for a missing file.\n");
    source.push_str("pub static CRAFT_I18N: &craft_i18n::Embedded = &[\n");
    for (code, shared, app) in entries {
        // `{:?}` writes the path as a Rust string literal, escaping Windows backslashes.
        let shared = shared.map_or_else(|| "\"\"".to_string(), |path| format!("include_str!({path:?})"));
        let _ = writeln!(source, "    ({code:?}, {shared}, include_str!({app:?})),");
    }
    source.push_str("];\n");
    source
}

/// One embedded locale: its code, its `common.tsv` if it has one, and its encoderCraft catalog.
type Locale = (String, Option<PathBuf>, PathBuf);

/// Every locale under `dir` with encoderCraft's catalog, by code. An error when `dir` isn't a craft-i18n `locales/`
/// directory.
fn locales(dir: &Path) -> Result<Vec<Locale>, String> {
    // Build scripts run in the crate's directory and `include_str!` resolves against the generated file, so a relative
    // path would mean two different places.
    if !dir.is_absolute() {
        return Err(format!("{} is not an absolute path", dir.display()));
    }
    // Before read_dir, so a checkout that appears later reruns the script.
    println!("cargo::rerun-if-changed={}", dir.display());
    let list = std::fs::read_dir(dir).map_err(|error| format!("{} is not a craft-i18n checkout ({error})", dir.display()))?;
    let mut out = Vec::new();
    for entry in list.flatten() {
        let path = entry.path();
        let Some(code) = path.file_name().and_then(|name| name.to_str()).map(str::to_string) else { continue };
        let app = path.join(APP_CATALOG);
        if !app.is_file() {
            continue;
        }
        println!("cargo::rerun-if-changed={}", app.display());
        let shared = Some(path.join("common.tsv")).filter(|shared| shared.is_file());
        if let Some(shared) = &shared {
            println!("cargo::rerun-if-changed={}", shared.display());
        }
        out.push((code, shared, app));
    }
    if out.is_empty() {
        return Err(format!("{} has no locale with an {APP_CATALOG}", dir.display()));
    }
    out.sort();
    Ok(out)
}
