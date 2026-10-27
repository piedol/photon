//! Photon for Windows: screenshots, markup and screen recording.
//!
//! Platform-free logic is shared with the Mac app by including its source files
//! unchanged (`#[path]` below); everything that touches the OS is Windows-native.

#![windows_subsystem = "windows"]

#[cfg(not(all(target_os = "windows", target_arch = "x86_64")))]
compile_error!("This is the Windows edition of Photon (x86_64-pc-windows-msvc). The Mac app lives at the repo root.");

// Shared with the Mac app (read-only includes). Some of their helpers are
// Mac-only, so dead-code warnings are silenced for these modules.
#[path = "../../src/commands.rs"]
#[allow(dead_code)]
mod commands;
#[path = "../../src/output/destination.rs"]
#[allow(dead_code)]
mod destination;
#[path = "../../src/geometry.rs"]
#[allow(dead_code)]
mod geometry;
#[path = "../../src/annotate/model.rs"]
#[allow(dead_code)]
mod model;
#[path = "../../src/output/naming.rs"]
#[allow(dead_code)]
mod naming;
#[path = "../../src/overlay/selection.rs"]
#[allow(dead_code)]
mod selection;
#[path = "../../src/i18n_text.rs"]
mod i18n_text;

// Windows-native.
mod app;
mod capture;
mod cloud;
mod d2d;
mod export;
mod hotkey;
mod hud;
mod i18n;
mod keys;
mod overlay;
mod pin;
mod record;
mod render;
mod selftest;
mod session;
mod settings;
mod settings_window;
mod toolbar;
mod util;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(app::main(args));
}
