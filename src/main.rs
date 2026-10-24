//! Photon: screenshots, markup and screen recording for Apple silicon Macs.

#![cfg_attr(debug_assertions, allow(dead_code))]

mod annotate;
mod app;
mod capture;
mod commands;
mod export;
mod geometry;
mod hotkey;
mod hud;
mod keys;
mod output;
mod overlay;
mod permissions;
mod pin;
mod record;
mod selftest;
mod session;
mod settings;
mod ui;
mod util;

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
compile_error!("Photon is built for Apple silicon Macs only (aarch64-apple-darwin).");

fn main() {
    app::run();
}
