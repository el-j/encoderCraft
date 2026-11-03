//! Windows only: embed the app icon and version info (VERSIONINFO) into `encodercraft.exe`, so it
//! shows in Explorer, the taskbar, the Start menu and Alt-Tab.
//!
//! On every other target this does nothing. A missing resource compiler is a warning, so a
//! cross-compile from macOS or Linux still links, unless `ENCODERCRAFT_REQUIRE_WINRES=1` turns it
//! into an error (for release builds).

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/app-icon/encodercraft.ico");
    println!("cargo:rerun-if-env-changed=ENCODERCRAFT_REQUIRE_WINRES");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("../../assets/app-icon/encodercraft.ico")
        .set("ProductName", "EncoderCraft")
        .set("FileDescription", "EncoderCraft media transcoder")
        .set("CompanyName", "Learning Machines LLC")
        .set("LegalCopyright", "Copyright (c) the EncoderCraft authors. MIT OR Apache-2.0.")
        .set("OriginalFilename", "encodercraft.exe")
        .set("InternalName", "encodercraft");
    if let Err(e) = res.compile() {
        if std::env::var_os("ENCODERCRAFT_REQUIRE_WINRES").is_some() {
            eprintln!("embedding Windows resources failed: {e}");
            std::process::exit(1);
        }
        println!("cargo:warning=encodercraft.exe built without icon/version resources: {e}");
    }
}
