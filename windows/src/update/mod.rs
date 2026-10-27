//! In-app updates from the repository's latest GitHub release: the Windows counterpart of
//! the Mac's `src/update/`, with the same calls, through Windows' own APIs. WinHTTP fetches
//! (with Windows' TLS and proxy settings), BCrypt hashes, `tar.exe` (part of Windows 10 and
//! 11) unpacks, and the exe is swapped by renaming, since Windows lets a running exe be
//! renamed but not overwritten.
//!
//! - [`check`]: asks GitHub for the latest release; `done` gets the answer on the UI thread.
//! - [`install`]: downloads the release's zip, verifies it, waits until `hooks.is_idle()`,
//!   renames the running exe to `Photon.exe.old` and puts the new one in its place.
//!   `hooks.done(Ok(()))` means the app should quit now, and call [`start_pending`] last
//!   thing, once its tray icon, shortcuts and single-instance lock are gone, so the new
//!   version (which may not know `--updated`) doesn't find this one still running.
//!   [`Error::NotWritable`] means it should open `release.page` instead. Any error leaves
//!   the current exe in place.
//! - [`launched`]: call once at launch. Removes `Photon.exe.old` and what an interrupted
//!   install left behind, and returns the version when this launch follows an update.
//! - [`wait_for_exit`]: the new exe gets `--updated <pid>` and waits for the old one to quit
//!   before it takes the single-instance lock.
//!
//! Photon.exe isn't Authenticode-signed, so the check is the zip's SHA-256 against the
//! `digest` GitHub lists for it, both fetched over HTTPS (no digest, no install). The exe
//! inside must name itself Photon in its version resource and be newer than the running one.
//!
//! Shared with the Mac unchanged: version numbers, reading the release JSON, release notes
//! and the schedule (the `#[path]` modules below).
//!
//! Testing: `<PREFIX>_UPDATE_FEED=<path or file:// URL>` reads the release JSON from a file
//! instead of GitHub, and that feed may point at a file:// zip.

mod fetch;
mod install;
#[path = "../../../src/update/notes.rs"]
pub mod notes;
#[path = "../../../src/update/release.rs"]
mod release;
#[path = "../../../src/update/schedule.rs"]
pub mod schedule;
mod verify;
#[path = "../../../src/update/version.rs"]
mod version;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use release::Release;
pub use version::Version;

use crate::util;

pub struct Config {
    /// "owner/name" on GitHub.
    pub repo: &'static str,
    /// The release asset to install, e.g. "Photon-windows-x64.zip". It holds `<name>.exe`.
    pub asset: &'static str,
    /// The app's name: the exe in the zip, the ProductName it must carry, and
    /// %LOCALAPPDATA%\<name> for downloads.
    pub name: &'static str,
    /// Prefix of the test overrides: "PHOTON" reads PHOTON_UPDATE_FEED.
    pub env_prefix: &'static str,
    /// The running version: env!("CARGO_PKG_VERSION").
    pub version: &'static str,
    pub log: fn(&str),
}

impl Config {
    fn feed_override(&self) -> Option<String> {
        std::env::var(format!("{}_UPDATE_FEED", self.env_prefix)).ok().filter(|f| !f.is_empty())
    }

    fn feed(&self) -> String {
        self.feed_override().unwrap_or_else(|| format!("https://api.github.com/repos/{}/releases/latest", self.repo))
    }

    fn user_agent(&self) -> String {
        format!("{}/{} (+https://github.com/{})", self.name, current_version(self), self.repo)
    }

    fn cache(&self) -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| util::home().join("AppData\\Local"));
        base.join(self.name)
    }

    fn staging(&self) -> PathBuf {
        self.cache().join("Updates")
    }

    fn marker(&self) -> PathBuf {
        self.cache().join("just-updated")
    }
}

#[derive(Clone, Debug)]
pub enum Check {
    Available(Release),
    UpToDate,
}

/// Where an install has got to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    /// Bytes so far, of `total` (0 until the server says).
    Downloading { received: u64, total: u64 },
    Verifying,
    /// Verified, and waiting for `hooks.is_idle()`.
    Waiting,
    Replacing,
}

/// The app's side of an install. All three run on the UI thread.
#[derive(Clone, Copy)]
pub struct Hooks {
    /// Whether the app can quit now without losing anything.
    pub is_idle: fn() -> bool,
    pub progress: fn(Step),
    /// Ok: the new exe is in place. Quit now, then call [`start_pending`].
    pub done: fn(Result<(), Error>),
}

#[derive(Clone, Debug)]
pub enum Error {
    /// No answer: offline, DNS, TLS or a timeout. Carries Windows' description.
    Network(String),
    /// GitHub answered with this HTTP status instead of a release.
    Http(u16),
    /// The answer wasn't a usable release.
    Release(String),
    /// The download failed a check, so nothing was replaced.
    Verify(String),
    /// This user can't change the folder the exe is in. Open the release page instead.
    NotWritable(String),
    /// The download couldn't be stored, or swapping the exes failed. The current exe is in place.
    Replace(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Network(why) => write!(f, "Couldn't reach GitHub. {why}"),
            Error::Http(403 | 429) => f.write_str("GitHub is limiting requests from this network. Try again in an hour."),
            Error::Http(404) => f.write_str("GitHub has no published release to compare with."),
            Error::Http(code) => write!(f, "GitHub answered with HTTP status {code}. Try again later."),
            Error::Release(s) | Error::Verify(s) | Error::NotWritable(s) | Error::Replace(s) => f.write_str(s),
        }
    }
}

/// For the shared schedule: run `f` on the UI thread after `wait`.
fn main_after(wait: Duration, f: impl FnOnce() + 'static) {
    util::main_after(wait.as_secs_f64(), f);
}

/// The running version.
pub fn current_version(cfg: &Config) -> Version {
    Version::parse(cfg.version).unwrap_or_else(|| Version::parse("0").unwrap())
}

/// Ask GitHub for the latest release. `done` runs on the UI thread.
pub fn check(cfg: &'static Config, done: impl FnOnce(Result<Check, Error>) + Send + 'static) {
    let current = current_version(cfg);
    let feed = cfg.feed();
    let allow_file = cfg.feed_override().is_some();
    (cfg.log)(&format!("update: checking {feed}"));
    let agent = cfg.user_agent();
    std::thread::spawn(move || {
        let answer = fetch::get(&feed, &agent, "application/vnd.github+json", allow_file);
        let result = answer.and_then(|body| release::parse(&body, cfg.asset, cfg.repo, allow_file)).map(|r| match r {
            Some(r) if r.version > current => Check::Available(r),
            _ => Check::UpToDate,
        });
        match &result {
            Ok(Check::Available(r)) => (cfg.log)(&format!("update: {} is available (running {current})", r.version)),
            Ok(Check::UpToDate) => (cfg.log)(&format!("update: {current} is the latest")),
            Err(e) => (cfg.log)(&format!("update: check failed: {e}")),
        }
        util::main_async(move || done(result));
    });
}

static INSTALLING: AtomicBool = AtomicBool::new(false);

/// Whether an install is under way (or finished and waiting for the app to quit).
pub fn installing() -> bool {
    INSTALLING.load(Ordering::SeqCst)
}

/// Bytes received and expected, written by the download thread and read for progress.
#[derive(Default)]
pub struct Progress {
    pub received: AtomicU64,
    pub total: AtomicU64,
    pub arrived: AtomicBool,
}

/// Download, verify and install `release`, then start the new exe. UI thread; see the module docs.
pub fn install(cfg: &'static Config, release: Release, hooks: Hooks) {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return;
    }
    (cfg.log)(&format!("update: installing {} from {}", release.version, release.url));
    let ready = running_exe(cfg).and_then(|exe| {
        let folder = exe.parent().unwrap_or(&exe).to_path_buf();
        if !install::writable(&folder) {
            return Err(Error::NotWritable(format!(
                "{} can't replace itself because your account can't make changes in {}.",
                cfg.name,
                folder.display()
            )));
        }
        let digest = release.digest.as_deref().ok_or_else(|| {
            Error::Verify(format!("The release doesn't list a checksum for {}, so the download can't be checked.", cfg.asset))
        })?;
        let expected = release::parse_digest(digest)
            .ok_or_else(|| Error::Verify(format!("The release's checksum for {} isn't a SHA-256 digest.", cfg.asset)))?;
        Ok((exe, expected, fresh_staging(cfg)?))
    });
    let (exe, expected, staging) = match ready {
        Ok(r) => r,
        Err(e) => return finish(cfg, hooks, false, Err(e)),
    };
    let zip = staging.join(cfg.asset);
    let progress = Arc::new(Progress::default());
    let current = current_version(cfg);
    let (url, agent, allow_file) = (release.url.clone(), cfg.user_agent(), cfg.feed_override().is_some());
    let seen = progress.clone();
    std::thread::spawn(move || {
        let downloaded = fetch::download(&url, &agent, &zip, &seen, allow_file);
        seen.arrived.store(true, Ordering::SeqCst);
        util::main_async(move || (hooks.progress)(Step::Verifying));
        let verified = downloaded.and_then(|()| verify_download(cfg, &zip, &staging, &expected, &current));
        util::main_async(move || match verified {
            Ok(new) => when_idle(cfg, hooks, exe, new),
            Err(e) => finish(cfg, hooks, true, Err(e)),
        });
    });
    poll_download(progress, hooks.progress);
}

fn poll_download(progress: Arc<Progress>, report: fn(Step)) {
    if progress.arrived.load(Ordering::SeqCst) {
        return;
    }
    let received = progress.received.load(Ordering::SeqCst);
    let total = progress.total.load(Ordering::SeqCst);
    report(Step::Downloading { received, total });
    util::main_after(0.25, move || poll_download(progress, report));
}

fn running_exe(cfg: &Config) -> Result<PathBuf, Error> {
    std::env::current_exe()
        .map_err(|e| Error::Replace(format!("{} couldn't find its own exe: {e}.", cfg.name)))
}

/// An empty folder for this download, after clearing out earlier ones.
fn fresh_staging(cfg: &Config) -> Result<PathBuf, Error> {
    let _ = std::fs::remove_dir_all(cfg.staging());
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let dir = cfg.staging().join(secs.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| Error::Replace(format!("The download folder couldn't be made: {e}.")))?;
    Ok(dir)
}

/// The checks, in order. Returns the unpacked exe.
fn verify_download(cfg: &Config, zip: &Path, staging: &Path, expected: &[u8; 32], current: &Version) -> Result<PathBuf, Error> {
    let log = |m: String| (cfg.log)(&format!("update: {m}"));
    let actual = verify::sha256_file(zip).map_err(|e| Error::Verify(format!("The download couldn't be read: {e}.")))?;
    if actual != *expected {
        return Err(Error::Verify("The download doesn't match the checksum GitHub lists for it.".into()));
    }
    log("checksum matches".into());
    let unpacked = staging.join("unpacked");
    verify::unzip(zip, &unpacked).map_err(|e| {
        log(e);
        Error::Verify("The download couldn't be unpacked.".into())
    })?;
    let exe = unpacked.join(format!("{}.exe", cfg.name));
    if !std::fs::symlink_metadata(&exe).is_ok_and(|m| m.is_file()) {
        return Err(Error::Verify(format!("The download doesn't contain {}.exe.", cfg.name)));
    }
    let (product, version) =
        verify::exe_info(&exe).ok_or_else(|| Error::Verify("The downloaded app's version couldn't be read.".into()))?;
    if product != cfg.name {
        return Err(Error::Verify(format!("The download isn't {}.", cfg.name)));
    }
    if Version::parse(&version).is_none_or(|v| v <= *current) {
        return Err(Error::Verify(format!("The downloaded app is version {version}, which isn't newer than {current}.")));
    }
    log(format!("{} {version} checks out", cfg.name));
    Ok(exe)
}

/// The exe to start when this process quits, once an update has put it in place.
static PENDING: Mutex<Option<PathBuf>> = Mutex::new(None);

/// Swap the exes once the app is idle. The new one starts when this process quits.
fn when_idle(cfg: &'static Config, hooks: Hooks, exe: PathBuf, new: PathBuf) {
    if !(hooks.is_idle)() {
        (hooks.progress)(Step::Waiting);
        return util::main_after(1.0, move || when_idle(cfg, hooks, exe, new));
    }
    (hooks.progress)(Step::Replacing);
    let version = verify::exe_info(&new).map(|(_, v)| v).unwrap_or_default();
    let result = install::replace(&exe, &new).map_err(Error::Replace).map(|old| {
        (cfg.log)(&format!("update: the previous version is at {}", old.display()));
        let _ = std::fs::write(cfg.marker(), &version);
        *PENDING.lock().unwrap() = Some(exe);
    });
    finish(cfg, hooks, true, result);
}

/// Whether an update is in place and waiting for this process to quit.
pub fn pending() -> bool {
    PENDING.lock().unwrap().is_some()
}

/// Start the new version, with `--updated <this pid>` and without the test overrides.
/// Call last thing before exiting, after releasing what a second copy would trip over.
pub fn start_pending(cfg: &Config) {
    let Some(exe) = PENDING.lock().unwrap().take() else { return };
    let clear = [format!("{}_UPDATE_TEST", cfg.env_prefix), format!("{}_UPDATE_FEED", cfg.env_prefix)];
    match install::start(&exe, &clear) {
        Ok(()) => (cfg.log)("update: started the new version"),
        // The new exe is in place; the next start runs it.
        Err(e) => (cfg.log)(&format!("update: couldn't start the new version: {e}")),
    }
}

fn finish(cfg: &Config, hooks: Hooks, staged: bool, result: Result<(), Error>) {
    if staged {
        let _ = std::fs::remove_dir_all(cfg.staging());
    }
    match &result {
        Ok(()) => (cfg.log)("update: installed; the new version starts when this one quits"),
        Err(e) => {
            (cfg.log)(&format!("update: not installed: {e}"));
            INSTALLING.store(false, Ordering::SeqCst);
        }
    }
    (hooks.done)(result);
}

/// Call once at launch: removes the previous exe and what an interrupted install left behind,
/// and returns the running version when this launch follows an update to it.
pub fn launched(cfg: &'static Config) -> Option<Version> {
    let _ = std::fs::remove_dir_all(cfg.staging());
    if let Ok(exe) = std::env::current_exe() {
        std::thread::spawn(move || install::remove_old(&exe, cfg.log));
    }
    let marker = cfg.marker();
    let written = std::fs::read_to_string(&marker).ok()?;
    let _ = std::fs::remove_file(&marker);
    let current = current_version(cfg);
    Version::parse(&written).filter(|v| *v == current)
}

/// `--updated <pid>`: wait up to 30 seconds for the version that started this one to quit.
pub fn wait_for_exit(pid: u32) {
    install::wait_for_exit(pid, Duration::from_secs(30));
}
