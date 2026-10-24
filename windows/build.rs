//! Embeds the app manifest (per-monitor DPI awareness, modern common controls)
//! and the icon. Skipped when type-checking from a non-Windows host.

fn main() {
    println!("cargo:rerun-if-changed=resources/photon.rc");
    println!("cargo:rerun-if-changed=resources/photon.manifest");
    println!("cargo:rerun-if-changed=resources/photon.ico");
    let target_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if target_windows && cfg!(windows) {
        embed_resource::compile("resources/photon.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to embed resources");
    }
}
