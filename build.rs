//! Embeds the icon, version information and application manifest into
//! Expunge.exe when building for Windows.

fn main() {
    println!("cargo:rerun-if-changed=assets/expunge.ico");
    println!("cargo:rerun-if-changed=assets/expunge.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let version = env!("CARGO_PKG_VERSION");
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/expunge.ico")
        .set_manifest_file("assets/expunge.manifest")
        .set("ProductName", "Expunge")
        .set(
            "FileDescription",
            "Expunge – install monitor and complete uninstaller",
        )
        .set("CompanyName", "Sean Shaffer")
        .set(
            "LegalCopyright",
            "Copyright © 2026 Sean Shaffer. Licensed under GPLv3.",
        )
        .set("OriginalFilename", "Expunge.exe")
        .set("InternalName", "Expunge")
        .set("ProductVersion", version)
        .set("FileVersion", version);
    if let Err(e) = res.compile() {
        panic!("failed to embed Windows resources: {e}");
    }
}
