//! Screen recording (filled in during phase 5).

use crate::session::RecordTarget;

pub fn is_active() -> bool {
    false
}

pub fn start(_t: RecordTarget) {
    crate::hud::message("Recording is coming in the next build", None);
}
