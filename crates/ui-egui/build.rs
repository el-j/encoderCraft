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
