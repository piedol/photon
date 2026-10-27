//! Every user action has a `Command`. Buttons and menu items carry it as their tag
//! and send `command:` to the shared controller, which routes here.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    // Menu bar
    CaptureScreenshot,
    RecordScreen,
    RecordHotkey,
    StopRecording,
    PauseResume,
    OpenRecordings,
    OpenSettings,
    ToggleLogin,
    Quit,
    Escape,
    // Capture overlay
    Tool(u8),
    Color(u8),
    CustomColor,
    ToggleSwatches,
    CycleStroke,
    Stroke(u8),
    Copy,
    Save,
    Print,
    Pin,
    RecordSelection,
    Undo,
    Close,
    // Record bar
    ToggleMic,
    ToggleSystemAudio,
    ToggleWebcam,
    StartRecording,
    SkipCountdown,
    // Finished-recording HUD
    Reveal,
    CopyFile,
    OpenFile,
    Trim,
    ExportGif,
    DismissHud,
    // Updates: 0 the menu item, then the update window's buttons (see update_ui)
    Update(u8),
    // Settings window controls
    Setting(u16),
}

impl Command {
    pub fn tag(self) -> isize {
        use Command::*;
        match self {
            CaptureScreenshot => 1,
            RecordScreen => 2,
            RecordHotkey => 3,
            StopRecording => 4,
            PauseResume => 5,
            OpenRecordings => 6,
            OpenSettings => 7,
            ToggleLogin => 8,
            Quit => 9,
            Escape => 10,
            Tool(i) => 100 + i as isize,
            Color(i) => 200 + i as isize,
            CustomColor => 250,
            ToggleSwatches => 251,
            CycleStroke => 252,
            Stroke(i) => 260 + i as isize,
            Copy => 300,
            Save => 301,
            Print => 302,
            Pin => 303,
            RecordSelection => 304,
            Undo => 305,
            Close => 306,
            ToggleMic => 400,
            ToggleSystemAudio => 401,
            ToggleWebcam => 402,
            StartRecording => 403,
            SkipCountdown => 404,
            Reveal => 500,
            CopyFile => 501,
            OpenFile => 502,
            Trim => 503,
            ExportGif => 504,
            DismissHud => 505,
            Update(i) => 600 + i as isize,
            Setting(i) => 1000 + i as isize,
        }
    }

    pub fn from_tag(tag: isize) -> Option<Self> {
        use Command::*;
        Some(match tag {
            1 => CaptureScreenshot,
            2 => RecordScreen,
            3 => RecordHotkey,
            4 => StopRecording,
            5 => PauseResume,
            6 => OpenRecordings,
            7 => OpenSettings,
            8 => ToggleLogin,
            9 => Quit,
            10 => Escape,
            100..=199 => Tool((tag - 100) as u8),
            200..=249 => Color((tag - 200) as u8),
            250 => CustomColor,
            251 => ToggleSwatches,
            252 => CycleStroke,
            260..=269 => Stroke((tag - 260) as u8),
            300 => Copy,
            301 => Save,
            302 => Print,
            303 => Pin,
            304 => RecordSelection,
            305 => Undo,
            306 => Close,
            400 => ToggleMic,
            401 => ToggleSystemAudio,
            402 => ToggleWebcam,
            403 => StartRecording,
            404 => SkipCountdown,
            500 => Reveal,
            501 => CopyFile,
            502 => OpenFile,
            503 => Trim,
            504 => ExportGif,
            505 => DismissHud,
            600..=609 => Update((tag - 600) as u8),
            1000..=1999 => Setting((tag - 1000) as u16),
            _ => return None,
        })
    }
}

/// Route a command to the module that owns it.
pub fn run(cmd: Command) {
    crate::util::log(format!("command {cmd:?}"));
    crate::app::handle(cmd);
}

#[cfg(test)]
mod tests {
    use super::Command::*;
    use super::*;

    #[test]
    fn tags_round_trip() {
        let all = [
            CaptureScreenshot, RecordScreen, RecordHotkey, StopRecording, PauseResume,
            OpenRecordings, OpenSettings, ToggleLogin, Quit, Escape, Tool(0), Tool(9), Color(7),
            CustomColor, ToggleSwatches, CycleStroke, Stroke(2), Copy, Save, Print, Pin, RecordSelection, Undo, Close,
            ToggleMic, ToggleSystemAudio, ToggleWebcam, StartRecording, SkipCountdown, Reveal, CopyFile,
            OpenFile, Trim, ExportGif, DismissHud, Update(0), Update(3), Setting(42),
        ];
        for c in all {
            assert_eq!(Command::from_tag(c.tag()), Some(c), "{c:?}");
        }
    }
}
