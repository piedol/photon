//! Small helpers: wide strings, paths, logging, and running work on the UI thread.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, PostMessageW, SetTimer, WM_APP};
use windows::core::PCWSTR;

/// Posted to the message window to drain queued closures.
pub const WM_RUN_QUEUED: u32 = WM_APP + 1;

static MESSAGE_HWND: AtomicIsize = AtomicIsize::new(0);
type Job = Box<dyn FnOnce() + Send>;
static QUEUE: Mutex<VecDeque<Job>> = Mutex::new(VecDeque::new());

pub fn set_message_window(hwnd: HWND) {
    MESSAGE_HWND.store(hwnd.0 as isize, Ordering::SeqCst);
}

pub fn message_window() -> HWND {
    HWND(MESSAGE_HWND.load(Ordering::SeqCst) as *mut _)
}

/// Run `f` on the UI thread soon (safe to call from any thread).
pub fn main_async(f: impl FnOnce() + Send + 'static) {
    QUEUE.lock().unwrap().push_back(Box::new(f));
    let hwnd = message_window();
    if !hwnd.0.is_null() {
        unsafe {
            let _ = PostMessageW(Some(hwnd), WM_RUN_QUEUED, WPARAM(0), LPARAM(0));
        }
    }
}

/// Called by the message window on `WM_RUN_QUEUED`.
pub fn drain_queue() {
    loop {
        let job = QUEUE.lock().unwrap().pop_front();
        match job {
            Some(job) => job(),
            None => break,
        }
    }
}

thread_local! {
    static TIMERS: RefCell<HashMap<usize, Box<dyn FnOnce()>>> = RefCell::new(HashMap::new());
}
static NEXT_TIMER: AtomicUsize = AtomicUsize::new(1000);

/// Run `f` on the UI thread after `secs` seconds. Must be called on the UI thread.
pub fn main_after(secs: f64, f: impl FnOnce() + 'static) {
    let id = NEXT_TIMER.fetch_add(1, Ordering::Relaxed);
    TIMERS.with(|t| t.borrow_mut().insert(id, Box::new(f)));
    unsafe {
        SetTimer(Some(message_window()), id, (secs * 1000.0).max(1.0) as u32, None);
    }
}

/// Called by the message window on `WM_TIMER`. Returns false if `id` isn't ours.
pub fn fire_timer(id: usize) -> bool {
    let job = TIMERS.with(|t| t.borrow_mut().remove(&id));
    match job {
        Some(job) => {
            unsafe {
                let _ = KillTimer(Some(message_window()), id);
            }
            job();
            true
        }
        None => false,
    }
}

/// Wraps a UI-thread value so it can ride through a `Send` closure that is
/// guaranteed to run back on the UI thread.
pub struct MainBound<T>(pub T);
// SAFETY: only used for values created on and consumed on the UI thread.
unsafe impl<T> Send for MainBound<T> {}

/// `MAKEINTRESOURCEW`: a resource ID passed where a name pointer is expected.
pub fn int_resource(id: u16) -> PCWSTR {
    PCWSTR(id as usize as *const u16)
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A NUL-terminated wide string that lives as long as the returned value.
pub struct W(pub Vec<u16>);
impl W {
    pub fn new(s: &str) -> W {
        W(wide(s))
    }
    pub fn p(&self) -> PCWSTR {
        PCWSTR(self.0.as_ptr())
    }
}

pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub fn home() -> PathBuf {
    std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\"))
}

/// `~\…` display form of a path (Photon's shared code calls this `tilde`).
pub fn tilde(path: &Path) -> String {
    let home = home();
    match path.strip_prefix(&home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
        Ok(rest) => format!("~\\{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// `%LOCALAPPDATA%\Photon`, created on demand.
pub fn app_data() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| home().join("AppData\\Local"));
    let d = base.join("Photon");
    let _ = std::fs::create_dir_all(&d);
    d
}

pub fn log_enabled() -> bool {
    cfg!(debug_assertions) || std::env::var_os("PHOTON_LOG").is_some()
}

/// Logs to stderr and `%LOCALAPPDATA%\Photon\photon.log` when PHOTON_LOG is set.
pub fn log(msg: impl AsRef<str>) {
    if !log_enabled() {
        return;
    }
    let line = format!("[photon] {}\n", msg.as_ref());
    eprint!("{line}");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(app_data().join("photon.log")) {
        let _ = f.write_all(line.as_bytes());
    }
}

static BUSY: AtomicUsize = AtomicUsize::new(0);

/// Held while Photon does something quitting would lose: saving a recording, trimming,
/// making a GIF, or waiting in a file or print dialog. An update waits until none is held.
pub struct Busy(());

impl Busy {
    pub fn start() -> Busy {
        BUSY.fetch_add(1, Ordering::SeqCst);
        Busy(())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn busy() -> bool {
    BUSY.load(Ordering::SeqCst) > 0
}

/// Give freed capture buffers back to the system shortly after a session ends.
pub fn release_memory_soon() {
    main_after(1.0, || unsafe {
        use windows::Win32::System::Memory::{GetProcessHeap, HeapCompact, HEAP_FLAGS};
        use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        if let Ok(heap) = GetProcessHeap() {
            HeapCompact(heap, HEAP_FLAGS(0));
        }
        // (-1, -1) asks Windows to trim the working set now.
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    });
}
