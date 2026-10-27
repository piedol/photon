//! In-app updates from a GitHub repository's latest release. This folder is
//! shared by piedol's Mac apps and copied as it is; each app supplies a
//! [`Config`] and its own menu item, setting, alerts and notification.
//!
//! - [`Config`]: repository, asset, app name, bundle id, team ID, the prefix of
//!   the test overrides, and a log function. Usually a `static`.
//! - [`current_version`]: the running bundle's CFBundleShortVersionString.
//! - [`check`]: asks GitHub for the latest release. `done` gets
//!   [`Check::Available`] or [`Check::UpToDate`], or an [`Error`], on the main thread.
//! - [`install`]: downloads the release's zip, verifies it, waits until
//!   `hooks.is_idle()`, swaps the bundle (the old one goes to the Trash) and starts
//!   the relaunch. `hooks.progress` gets each [`Step`]. `hooks.done(Ok(()))` means the
//!   app should quit now. [`Error::NotWritable`] means the app should open
//!   `release.page` in the browser instead. Any error leaves the current app in place.
//! - [`launched`]: call once at launch. Clears what an interrupted install left
//!   behind, and returns the version when this launch follows an update.
//! - [`schedule::start`]: the automatic check, a minute after launch when a day
//!   has passed, then daily.
//! - [`notes::parse`]: release notes as headings, bullets and text to draw.
//!
//! Before anything is replaced, the zip's SHA-256 must match the release's
//! `digest` (no digest, no install), and the unpacked app must satisfy
//! `anchor apple generic and identifier "<bundle id>" and certificate
//! leaf[subject.OU] = "<team ID>" and notarized` under strict validation, and be
//! newer than the running app.
//!
//! Testing: `<PREFIX>_UPDATE_FEED=<url>` reads the release JSON from that URL
//! instead of GitHub; a file:// feed may point at a file:// zip.
//!
//! Messages are whole English sentences, ready to wrap in a translation function.
//! Needs objc2, objc2-foundation, block2, dispatch2 and serde_json.

mod fetch;
mod install;
pub mod notes;
mod release;
pub mod schedule;
mod verify;
mod version;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use dispatch2::{DispatchQueue, DispatchTime};
use objc2_foundation::{NSBundle, NSString, NSURLSessionDownloadTask};

pub use release::Release;
pub use version::Version;

pub struct Config {
    /// "owner/name" on GitHub.
    pub repo: &'static str,
    /// The release asset to install, e.g. "Tamka-macOS-arm64.zip". It holds `<name>.app`.
    pub asset: &'static str,
    /// The app's name: the bundle in the zip, and ~/Library/Caches/<name> for downloads.
    pub name: &'static str,
    /// The bundle and signing identifier the new app must have.
    pub bundle_id: &'static str,
    /// The Apple Developer team that must have signed it.
    pub team_id: &'static str,
    /// Prefix of the test overrides: "TAMKA" reads TAMKA_UPDATE_FEED.
    pub env_prefix: &'static str,
    /// The version to report outside an app bundle (`cargo run`): env!("CARGO_PKG_VERSION").
    pub fallback_version: &'static str,
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
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
        home.join("Library/Caches").join(self.name)
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

/// The app's side of an install. All three run on the main thread.
#[derive(Clone, Copy)]
pub struct Hooks {
    /// Whether the app can quit now without losing anything.
    pub is_idle: fn() -> bool,
    pub progress: fn(Step),
    /// Ok: the new app is in place and opens once this process exits. Quit now.
    pub done: fn(Result<(), Error>),
}

#[derive(Clone, Debug)]
pub enum Error {
    /// No answer: offline, DNS, TLS or a timeout. Carries macOS's description.
    Network(String),
    /// GitHub answered with this HTTP status instead of a release.
    Http(u16),
    /// The answer wasn't a usable release.
    Release(String),
    /// The download failed a check, so nothing was replaced.
    Verify(String),
    /// This user can't change the folder the app is in. Open the release page instead.
    NotWritable(String),
    /// Not running from the app bundle (a development build).
    NotBundled(String),
    /// The download couldn't be stored, or swapping the bundles failed. The current app is in place.
    Replace(String),
    /// The new app is in place, but it couldn't be reopened.
    Relaunch(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Network(why) => write!(f, "Couldn't reach GitHub. {why}"),
            Error::Http(403 | 429) => f.write_str("GitHub is limiting requests from this network. Try again in an hour."),
            Error::Http(404) => f.write_str("GitHub has no published release to compare with."),
            Error::Http(code) => write!(f, "GitHub answered with HTTP status {code}. Try again later."),
            Error::Release(s)
            | Error::Verify(s)
            | Error::NotWritable(s)
            | Error::NotBundled(s)
            | Error::Replace(s)
            | Error::Relaunch(s) => f.write_str(s),
        }
    }
}

fn main(f: impl FnOnce() + Send + 'static) {
    DispatchQueue::main().exec_async(f);
}

fn main_after(wait: Duration, f: impl FnOnce() + Send + 'static) {
    let when = DispatchTime::NOW.time(wait.as_nanos().min(i64::MAX as u128) as i64);
    let _ = DispatchQueue::main().after(when, f);
}

/// The running app's CFBundleShortVersionString, or `fallback_version` outside a bundle.
pub fn current_version(cfg: &Config) -> Version {
    NSBundle::mainBundle()
        .objectForInfoDictionaryKey(&NSString::from_str("CFBundleShortVersionString"))
        .and_then(|v| v.downcast::<NSString>().ok())
        .and_then(|v| Version::parse(&v.to_string()))
        .or_else(|| Version::parse(cfg.fallback_version))
        .unwrap_or_else(|| Version::parse("0").unwrap())
}

/// Ask GitHub for the latest release. `done` runs on the main thread.
pub fn check(cfg: &'static Config, done: impl FnOnce(Result<Check, Error>) + Send + 'static) {
    let current = current_version(cfg);
    let feed = cfg.feed();
    let allow_file = cfg.feed_override().is_some();
    (cfg.log)(&format!("update: checking {feed}"));
    fetch::get(&feed, &cfg.user_agent(), "application/vnd.github+json", move |answer| {
        let result = answer.and_then(|body| release::parse(&body, cfg.asset, cfg.repo, allow_file)).map(|r| match r {
            Some(r) if r.version > current => Check::Available(r),
            _ => Check::UpToDate,
        });
        match &result {
            Ok(Check::Available(r)) => (cfg.log)(&format!("update: {} is available (running {current})", r.version)),
            Ok(Check::UpToDate) => (cfg.log)(&format!("update: {current} is the latest")),
            Err(e) => (cfg.log)(&format!("update: check failed: {e}")),
        }
        main(move || done(result));
    });
}

static INSTALLING: AtomicBool = AtomicBool::new(false);

/// Whether an install is under way (or finished and waiting for the app to quit).
pub fn installing() -> bool {
    INSTALLING.load(Ordering::SeqCst)
}

/// Download, verify and install `release`, then relaunch. Main thread; see the module docs.
pub fn install(cfg: &'static Config, release: Release, hooks: Hooks) {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return;
    }
    (cfg.log)(&format!("update: installing {} from {}", release.version, release.url));
    let ready = running_bundle(cfg).and_then(|bundle| {
        if !install::writable(&bundle) {
            return Err(not_writable(cfg, &bundle));
        }
        let digest = release.digest.as_deref().ok_or_else(|| {
            Error::Verify(format!("The release doesn't list a checksum for {}, so the download can't be checked.", cfg.asset))
        })?;
        let expected = release::parse_digest(digest)
            .ok_or_else(|| Error::Verify(format!("The release's checksum for {} isn't a SHA-256 digest.", cfg.asset)))?;
        Ok((bundle, expected, fresh_staging(cfg)?))
    });
    let (bundle, expected, staging) = match ready {
        Ok(r) => r,
        Err(e) => return finish(cfg, hooks, false, Err(e)),
    };
    let zip = staging.join(cfg.asset);
    let arrived = Arc::new(AtomicBool::new(false));
    let current = current_version(cfg);
    let flag = arrived.clone();
    let task = fetch::download(&release.url, &cfg.user_agent(), zip.clone(), move |downloaded| {
        flag.store(true, Ordering::SeqCst);
        main(move || (hooks.progress)(Step::Verifying));
        // Hashing and unpacking take a moment: keep them off the session's queue.
        std::thread::spawn(move || {
            let verified = downloaded.and_then(|()| verify_download(cfg, &zip, &staging, &expected, &current));
            main(move || match verified {
                Ok(app) => when_idle(cfg, hooks, bundle, app),
                Err(e) => finish(cfg, hooks, true, Err(e)),
            });
        });
    });
    match task {
        Ok(task) => poll_download(task, arrived, hooks.progress),
        Err(e) => finish(cfg, hooks, true, Err(e)),
    }
}

fn poll_download(task: objc2::rc::Retained<NSURLSessionDownloadTask>, arrived: Arc<AtomicBool>, progress: fn(Step)) {
    if arrived.load(Ordering::SeqCst) {
        return;
    }
    let received = task.countOfBytesReceived().max(0) as u64;
    let total = task.countOfBytesExpectedToReceive().max(0) as u64;
    progress(Step::Downloading { received, total });
    main_after(Duration::from_millis(250), move || poll_download(task, arrived, progress));
}

/// The running bundle, if this is the app (not a development build).
fn running_bundle(cfg: &Config) -> Result<PathBuf, Error> {
    let main = NSBundle::mainBundle();
    let path = PathBuf::from(main.bundlePath().to_string());
    let is_app = path.extension().is_some_and(|e| e == "app")
        && main.bundleIdentifier().is_some_and(|id| id.to_string() == cfg.bundle_id);
    if is_app {
        Ok(path)
    } else {
        Err(Error::NotBundled(format!("{} isn't running from its app bundle, so it can't replace itself.", cfg.name)))
    }
}

fn not_writable(cfg: &Config, bundle: &Path) -> Error {
    let folder = bundle.parent().unwrap_or(bundle).display().to_string();
    if folder.contains("/AppTranslocation/") {
        Error::NotWritable(format!(
            "macOS is running {name} from a read-only copy, so it can't replace itself. Move {name} into the Applications folder and open it from there.",
            name = cfg.name
        ))
    } else {
        Error::NotWritable(format!("{} can't replace itself because your account can't make changes in {folder}.", cfg.name))
    }
}

/// An empty folder for this download, after clearing out earlier ones.
fn fresh_staging(cfg: &Config) -> Result<PathBuf, Error> {
    let _ = std::fs::remove_dir_all(cfg.staging());
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let dir = cfg.staging().join(secs.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| Error::Replace(format!("The download folder couldn't be made: {e}.")))?;
    Ok(dir)
}

/// The checks, in order. Returns the unpacked app.
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
    let app = unpacked.join(format!("{}.app", cfg.name));
    if !std::fs::symlink_metadata(&app).is_ok_and(|m| m.is_dir()) {
        return Err(Error::Verify(format!("The download doesn't contain {}.app.", cfg.name)));
    }
    verify::check_signature(&app, &verify::requirement(cfg.bundle_id, cfg.team_id)).map_err(|(status, why)| {
        log(format!("signature check failed ({status}): {why}"));
        Error::Verify(if status == verify::REQUIREMENT_FAILED {
            format!("The downloaded app isn't signed by {}'s developer and notarized by Apple.", cfg.name)
        } else {
            "The downloaded app's signature is missing or broken, so it may have been changed.".into()
        })
    })?;
    log("signature, team and notarization check out".into());
    let (id, version) = verify::bundle_info(&app)
        .ok_or_else(|| Error::Verify("The downloaded app's version couldn't be read.".into()))?;
    if id != cfg.bundle_id {
        return Err(Error::Verify(format!("The download isn't {}.", cfg.name)));
    }
    if Version::parse(&version).is_none_or(|v| v <= *current) {
        return Err(Error::Verify(format!("The downloaded app is version {version}, which isn't newer than {current}.")));
    }
    Ok(app)
}

/// Swap the bundles once the app is idle, then start the relaunch.
fn when_idle(cfg: &'static Config, hooks: Hooks, bundle: PathBuf, app: PathBuf) {
    if !(hooks.is_idle)() {
        (hooks.progress)(Step::Waiting);
        return main_after(Duration::from_secs(1), move || when_idle(cfg, hooks, bundle, app));
    }
    (hooks.progress)(Step::Replacing);
    let version = verify::bundle_info(&app).map(|(_, v)| v).unwrap_or_default();
    let result = install::replace(&bundle, &app).map_err(Error::Replace).and_then(|trashed| {
        (cfg.log)(&format!("update: the previous version is at {}", trashed.display()));
        let _ = std::fs::write(cfg.marker(), &version);
        install::relaunch_after_exit(&bundle).map_err(|e| {
            (cfg.log)(&format!("update: couldn't start the relaunch: {e}"));
            Error::Relaunch(format!("{} {version} is installed, but it couldn't reopen itself. Quit {} and open it again.", cfg.name, cfg.name))
        })
    });
    finish(cfg, hooks, true, result);
}

fn finish(cfg: &Config, hooks: Hooks, staged: bool, result: Result<(), Error>) {
    if staged {
        let _ = std::fs::remove_dir_all(cfg.staging());
    }
    match &result {
        Ok(()) => (cfg.log)("update: installed; relaunching"),
        Err(e) => {
            (cfg.log)(&format!("update: not installed: {e}"));
            INSTALLING.store(false, Ordering::SeqCst);
        }
    }
    (hooks.done)(result);
}

/// Call once at launch: clears what an interrupted install left behind, and
/// returns the running version when this launch follows an update to it.
pub fn launched(cfg: &Config) -> Option<Version> {
    let _ = std::fs::remove_dir_all(cfg.staging());
    let marker = cfg.marker();
    let written = std::fs::read_to_string(&marker).ok()?;
    let _ = std::fs::remove_file(&marker);
    let current = current_version(cfg);
    Version::parse(&written).filter(|v| *v == current)
}
