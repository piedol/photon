//! Recording audio: system sound (WASAPI loopback) and the microphone, each
//! converted to 48 kHz stereo 16-bit by Windows, mixed, and fed to the encoder
//! on the recording clock. Silence fills gaps (loopback sends nothing while the
//! system is quiet), so audio always lines up with video, including after pauses.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

use super::Clock;
use crate::util;

const RATE: u32 = 48_000;
const CHANNELS: usize = 2;
/// Keep at most half a second queued per source.
const MAX_QUEUE: usize = RATE as usize / 2 * CHANNELS;

type Queue = Arc<Mutex<VecDeque<i16>>>;

pub struct Mixer {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
}

impl Mixer {
    /// `sink` receives interleaved little-endian i16 PCM; it returns false until
    /// the encoder exists (samples are then simply not counted yet).
    pub fn start(
        clock: Arc<Clock>,
        system: bool,
        mic: bool,
        mic_device: Option<String>,
        mut sink: impl FnMut(&[u8]) -> bool + Send + 'static,
    ) -> Option<Mixer> {
        let stop = Arc::new(AtomicBool::new(false));
        let mut threads = Vec::new();
        let mut queues: Vec<Queue> = Vec::new();
        if system {
            let q: Queue = Arc::new(Mutex::new(VecDeque::new()));
            queues.push(q.clone());
            let st = stop.clone();
            threads.push(std::thread::spawn(move || {
                if let Err(e) = capture_loop(true, None, q, st) {
                    util::log(format!("system audio failed: {e}"));
                }
            }));
        }
        if mic {
            let q: Queue = Arc::new(Mutex::new(VecDeque::new()));
            queues.push(q.clone());
            let st = stop.clone();
            threads.push(std::thread::spawn(move || {
                if let Err(e) = capture_loop(false, mic_device, q, st) {
                    util::log(format!("microphone failed: {e}"));
                }
            }));
        }
        if queues.is_empty() {
            return None;
        }
        let st = stop.clone();
        threads.push(std::thread::spawn(move || {
            let mut sent: u64 = 0; // sample frames delivered
            let mut was_paused = false;
            while !st.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(10));
                if clock.paused.load(Ordering::Relaxed) {
                    // Drop what arrives while paused so resume doesn't replay it.
                    for q in &queues {
                        q.lock().unwrap().clear();
                    }
                    was_paused = true;
                    continue;
                }
                if was_paused {
                    was_paused = false;
                }
                let target = (clock.elapsed().as_secs_f64() * RATE as f64) as u64;
                if target <= sent {
                    continue;
                }
                let frames = (target - sent).min(RATE as u64) as usize;
                let mut mix = vec![0i32; frames * CHANNELS];
                for q in &queues {
                    let mut q = q.lock().unwrap();
                    let n = (frames * CHANNELS).min(q.len());
                    for (i, s) in q.drain(..n).enumerate() {
                        mix[i] += s as i32;
                    }
                }
                let mut bytes = Vec::with_capacity(mix.len() * 2);
                for v in mix {
                    bytes.extend_from_slice(&(v.clamp(i16::MIN as i32, i16::MAX as i32) as i16).to_le_bytes());
                }
                if sink(&bytes) {
                    sent += frames as u64;
                } else {
                    // Encoder not ready yet: count the time as already covered.
                    sent = target;
                }
            }
        }));
        Some(Mixer { stop, threads })
    }

    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads {
            let _ = t.join();
        }
    }
}

fn capture_loop(loopback: bool, device_id: Option<String>, q: Queue, stop: Arc<AtomicBool>) -> Result<(), Box<dyn std::error::Error>> {
    let _ = wasapi::initialize_mta();
    let enumerator = DeviceEnumerator::new()?;
    let device = if loopback {
        enumerator.get_default_device(&Direction::Render)?
    } else {
        let mut chosen = None;
        if let Some(id) = &device_id {
            for d in &enumerator.get_device_collection(&Direction::Capture)? {
                let d = d?;
                if d.get_id().ok().as_deref() == Some(id.as_str()) {
                    chosen = Some(d);
                    break;
                }
            }
        }
        match chosen {
            Some(d) => d,
            None => enumerator.get_default_device(&Direction::Capture)?,
        }
    };
    let mut client = device.get_iaudioclient()?;
    let format = WaveFormat::new(16, 16, &SampleType::Int, RATE as usize, CHANNELS, None);
    let (def_period, _min) = client.get_device_period()?;
    // Loopback must be polled; Windows converts to our format (autoconvert).
    let mode = StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: def_period * 4 };
    client.initialize_client(&format, &Direction::Capture, &mode)?;
    let capture = client.get_audiocaptureclient()?;
    client.start_stream()?;
    let mut buf: VecDeque<u8> = VecDeque::new();
    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(5));
        loop {
            match capture.get_next_packet_size()? {
                Some(0) | None => break,
                Some(_) => {
                    capture.read_from_device_to_deque(&mut buf)?;
                }
            }
        }
        if buf.len() >= 2 {
            let n = buf.len() / 2 * 2;
            let bytes: Vec<u8> = buf.drain(..n).collect();
            let mut qq = q.lock().unwrap();
            for c in bytes.as_chunks::<2>().0 {
                qq.push_back(i16::from_le_bytes([c[0], c[1]]));
            }
            let excess = qq.len().saturating_sub(MAX_QUEUE);
            qq.drain(..excess);
        }
    }
    let _ = client.stop_stream();
    Ok(())
}

/// Microphones for the Settings picker: (device id, name).
pub fn microphones() -> Vec<(String, String)> {
    let _ = wasapi::initialize_mta();
    let mut out = Vec::new();
    if let Ok(e) = DeviceEnumerator::new()
        && let Ok(coll) = e.get_device_collection(&Direction::Capture)
    {
        for d in &coll {
            if let Ok(d) = d
                && let (Ok(id), Ok(name)) = (d.get_id(), d.get_friendlyname())
            {
                out.push((id, name));
            }
        }
    }
    out
}
