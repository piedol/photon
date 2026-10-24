//! Photon: screenshots, markup and screen recording for Apple silicon Macs.

mod app;
mod commands;
mod hotkey;
mod keys;
mod settings;
mod ui;
mod util;

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
compile_error!("Photon is built for Apple silicon Macs only (aarch64-apple-darwin).");

fn main() {
    app::run();
}
