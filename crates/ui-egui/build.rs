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
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");

    let required = std::env::var_os("CRAFT_FONTS_REQUIRED").is_some();
    let mut source = String::from("static CRAFT_FONTS: &[CraftFont] = &[];\n");
    match std::env::var_os("CRAFT_FONTS_DIR").map(PathBuf::from) {
        Some(dir) => match craft_fonts(&dir, required) {
            Ok(generated) => source = generated,
            Err(error) if required => println!("cargo::error=CRAFT_FONTS_DIR={}: {error}", dir.display()),
            Err(error) => println!("cargo::warning=building without craft-fonts: CRAFT_FONTS_DIR={}: {error}", dir.display()),
        },
        None if required => println!("cargo::error=CRAFT_FONTS_REQUIRED is set but CRAFT_FONTS_DIR is missing"),
        None => {}
    }

    let Some(out_dir) = std::env::var_os("OUT_DIR").map(PathBuf::from) else {
        println!("cargo::error=OUT_DIR is missing");
        return;
    };
    let output = out_dir.join("craft_fonts.rs");
    if let Err(error) = std::fs::write(&output, source) {
        println!("cargo::error=writing {}: {error}", output.display());
    }
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

fn craft_fonts(dir: &Path, required: bool) -> Result<String, String> {
    let root = dir.canonicalize().map_err(|error| error.to_string())?;
    let manifest = root.join("fonts/manifest.txt");
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).map_err(|error| format!("{}: {error}", manifest.display()))?;
    let wasm = std::env::var("CARGO_CFG_TARGET_ARCH").is_ok_and(|arch| arch == "wasm32");
    let mut source = String::from("static CRAFT_FONTS: &[CraftFont] = &[\n");
    let mut included = Vec::new();

    for (line_number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, ..] = fields.as_slice() else {
            return Err(format!("malformed manifest line {}", line_number + 1));
        };
        if fields.len() != 8 {
            return Err(format!("manifest line {} must contain eight fields", line_number + 1));
        }
        if wasm && !WEB_FONTS.contains(&(*family, *style)) {
            continue;
        }

        let path = root.join(file).canonicalize().map_err(|error| format!("{file}: {error}"))?;
        if !path.starts_with(&root) || !path.is_file() {
            return Err(format!("font path is not a file inside craft-fonts: {file}"));
        }
        let path = path.to_str().ok_or_else(|| format!("font path is not valid UTF-8: {file}"))?;
        println!("cargo::rerun-if-changed={path}");
        let scripts: Vec<_> = scripts.split(',').map(|script| format!("{:?}", script.trim())).collect();
        writeln!(
            source,
            "    CraftFont {{ family: {family:?}, style: {style:?}, scripts: &[{}], bytes: include_bytes!({path:?}) }},",
            scripts.join(", ")
        )
        .map_err(|_| "could not generate Rust font entries".to_string())?;
        included.push((*family, *style));
    }
    source.push_str("];\n");

    if required {
        for &(family, style) in WEB_FONTS {
            if !included.contains(&(family, style)) {
                return Err(format!("required fallback face is missing: {family} {style}"));
            }
        }
    }
    Ok(source)
}
