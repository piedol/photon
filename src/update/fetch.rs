//! HTTP through macOS's own NSURLSession: an ephemeral session (no cookies, no
//! disk cache), so the app carries no HTTP or TLS code of its own. Completion
//! handlers run on the session's background queue.

use std::path::PathBuf;
use std::sync::Mutex;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2_foundation::{
    NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSString, NSURL, NSURLRequestCachePolicy, NSURLResponse,
    NSURLSession, NSURLSessionConfiguration, NSURLSessionDownloadTask,
};

use super::Error;

fn session() -> Retained<NSURLSession> {
    let config = NSURLSessionConfiguration::ephemeralSessionConfiguration();
    config.setTimeoutIntervalForRequest(30.0);
    config.setTimeoutIntervalForResource(15.0 * 60.0);
    config.setRequestCachePolicy(NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData);
    config.setURLCache(None);
    NSURLSession::sessionWithConfiguration(&config)
}

fn request(url: &str, user_agent: &str, accept: &str) -> Result<Retained<NSMutableURLRequest>, Error> {
    let u = NSURL::URLWithString(&NSString::from_str(url)).ok_or_else(|| Error::Release(format!("{url} isn't a URL.")))?;
    let r = NSMutableURLRequest::requestWithURL(&u);
    r.setValue_forHTTPHeaderField(Some(&NSString::from_str(user_agent)), &NSString::from_str("User-Agent"));
    r.setValue_forHTTPHeaderField(Some(&NSString::from_str(accept)), &NSString::from_str("Accept"));
    Ok(r)
}

/// The request's outcome apart from its body: a network error, or an HTTP status
/// other than 200. Non-HTTP responses (file URLs in tests) count as fine.
fn outcome(response: *mut NSURLResponse, error: *mut NSError) -> Result<(), Error> {
    if let Some(e) = unsafe { error.as_ref() } {
        return Err(Error::Network(e.localizedDescription().to_string()));
    }
    let status = unsafe { response.as_ref() }.and_then(|r| r.downcast_ref::<NSHTTPURLResponse>()).map(|r| r.statusCode());
    match status {
        Some(200) | None => Ok(()),
        Some(code) => Err(Error::Http(code as u16)),
    }
}

/// GET `url`; `done` gets the body, on a background queue.
pub fn get(url: &str, user_agent: &str, accept: &str, done: impl FnOnce(Result<Vec<u8>, Error>) + Send + 'static) {
    let r = match request(url, user_agent, accept) {
        Ok(r) => r,
        Err(e) => return done(Err(e)),
    };
    r.setValue_forHTTPHeaderField(Some(&NSString::from_str("2022-11-28")), &NSString::from_str("X-GitHub-Api-Version"));
    let done = Mutex::new(Some(done));
    let block = RcBlock::new(move |data: *mut NSData, response: *mut NSURLResponse, error: *mut NSError| {
        let Some(done) = done.lock().unwrap().take() else { return };
        let body = unsafe { data.as_ref() }.map(|d| d.to_vec()).unwrap_or_default();
        done(outcome(response, error).map(|()| body));
    });
    let s = session();
    // SAFETY: the block only captures Send values.
    let task = unsafe { s.dataTaskWithRequest_completionHandler(&r, &block) };
    task.resume();
    s.finishTasksAndInvalidate();
}

/// Download `url` to `dest`; `done` runs on a background queue once the file is
/// there. The task is returned so its byte counts can be read for progress.
pub fn download(
    url: &str,
    user_agent: &str,
    dest: PathBuf,
    done: impl FnOnce(Result<(), Error>) + Send + 'static,
) -> Result<Retained<NSURLSessionDownloadTask>, Error> {
    let r = request(url, user_agent, "application/octet-stream")?;
    let done = Mutex::new(Some(done));
    let block = RcBlock::new(move |location: *mut NSURL, response: *mut NSURLResponse, error: *mut NSError| {
        let Some(done) = done.lock().unwrap().take() else { return };
        let result = outcome(response, error).and_then(|()| {
            // The temporary file is deleted when this handler returns: move it now.
            let from = unsafe { location.as_ref() }
                .and_then(|l| l.path())
                .map(|p| PathBuf::from(p.to_string()))
                .ok_or_else(|| Error::Network("The download didn't arrive.".into()))?;
            std::fs::rename(&from, &dest)
                .or_else(|_| std::fs::copy(&from, &dest).map(|_| ()))
                .map_err(|e| Error::Network(format!("The download couldn't be saved: {e}.")))
        });
        done(result);
    });
    let s = session();
    // SAFETY: the block only captures Send values.
    let task = unsafe { s.downloadTaskWithRequest_completionHandler(&r, &block) };
    task.resume();
    s.finishTasksAndInvalidate();
    Ok(task)
}
