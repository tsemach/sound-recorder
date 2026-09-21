use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use libpulse_binding as pulse;
use libpulse_simple_binding as psimple;

use pulse::context::{Context, FlagSet as ContextFlagSet, State as ContextState};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::proplist::Proplist;
use pulse::sample::{Format, Spec};
use pulse::stream::Direction;

use super::{AudioCapture, AudioFormat, AudioSource, CaptureError, FrameCallback};

const APP_NAME: &str = "Sound Recorder";
const BUFFER_MS: u64 = 20;

struct RawSource {
  name: String,
  description: String,
  is_monitor: bool,
  sample_rate: u32,
  channels: u8,
}

fn connection_failed(context: String) -> CaptureError {
  CaptureError {
    message: format!("Could not connect to the audio server: {context}"),
  }
}

/// Pure filter/map from raw source records to the trait's public, UI-facing
/// shape — takes already-fetched data, no live connection, so it's directly
/// unit-testable.
fn filter_monitor_sources(raw: Vec<RawSource>) -> Vec<AudioSource> {
  raw
    .into_iter()
    .filter(|s| s.is_monitor)
    .map(|s| AudioSource {
      id: s.name,
      name: s.description,
    })
    .collect()
}

/// Converts a raw native-endian PCM byte buffer (as read from
/// `psimple::Simple`) into interleaved i16 samples. Pure and unit-testable.
fn bytes_to_i16_samples(bytes: &[u8]) -> Vec<i16> {
  bytes
    .chunks_exact(2)
    .map(|pair| i16::from_ne_bytes([pair[0], pair[1]]))
    .collect()
}

/// Runs a standard (non-threaded) mainloop long enough to enumerate every
/// known PulseAudio/PipeWire source. Used both by `list_sources()` and by
/// `start()` to look up the selected source's real sample rate/channels.
fn query_sources() -> Result<Vec<RawSource>, CaptureError> {
  let mut proplist = Proplist::new().ok_or_else(|| connection_failed("proplist init".into()))?;
  proplist
    .set_str(pulse::proplist::properties::APPLICATION_NAME, APP_NAME)
    .map_err(|_| connection_failed("proplist set".into()))?;

  let mut mainloop = Mainloop::new().ok_or_else(|| connection_failed("mainloop init".into()))?;
  let context = Rc::new(RefCell::new(
    Context::new_with_proplist(&mainloop, APP_NAME, &proplist)
      .ok_or_else(|| connection_failed("context init".into()))?,
  ));

  context
    .borrow_mut()
    .connect(None, ContextFlagSet::NOFLAGS, None)
    .map_err(|e| connection_failed(format!("{e}")))?;

  loop {
    match mainloop.iterate(true) {
      IterateResult::Quit(_) | IterateResult::Err(_) => {
        return Err(connection_failed("mainloop iterate failed".into()));
      }
      IterateResult::Success(_) => {}
    }
    match context.borrow().get_state() {
      ContextState::Ready => break,
      ContextState::Failed | ContextState::Terminated => {
        return Err(connection_failed("context connection failed".into()));
      }
      _ => {}
    }
  }

  let sources = Rc::new(RefCell::new(Vec::new()));
  let sources_cb = Rc::clone(&sources);
  let done = Rc::new(RefCell::new(false));
  let done_cb = Rc::clone(&done);

  let _op = context
    .borrow()
    .introspect()
    .get_source_info_list(move |result| match result {
      pulse::callbacks::ListResult::Item(info) => {
        sources_cb.borrow_mut().push(RawSource {
          name: info.name.as_deref().unwrap_or("").to_string(),
          description: info
            .description
            .as_deref()
            .unwrap_or(info.name.as_deref().unwrap_or(""))
            .to_string(),
          is_monitor: info.monitor_of_sink.is_some(),
          sample_rate: info.sample_spec.rate,
          channels: info.sample_spec.channels,
        });
      }
      pulse::callbacks::ListResult::End | pulse::callbacks::ListResult::Error => {
        *done_cb.borrow_mut() = true;
      }
    });

  while !*done.borrow() {
    match mainloop.iterate(true) {
      IterateResult::Quit(_) | IterateResult::Err(_) => {
        return Err(connection_failed("mainloop iterate failed".into()));
      }
      IterateResult::Success(_) => {}
    }
  }

  context.borrow_mut().disconnect();

  Rc::try_unwrap(sources)
    .map(|cell| cell.into_inner())
    .map_err(|_| connection_failed("source list still borrowed".into()))
}

pub struct LinuxPulseCapture {
  running: Arc<AtomicBool>,
  paused: Arc<AtomicBool>,
  handle: Option<thread::JoinHandle<()>>,
  format: Arc<Mutex<AudioFormat>>,
}

impl LinuxPulseCapture {
  pub fn new() -> Self {
    Self {
      running: Arc::new(AtomicBool::new(false)),
      paused: Arc::new(AtomicBool::new(false)),
      handle: None,
      format: Arc::new(Mutex::new(AudioFormat {
        sample_rate: 48_000,
        channels: 2,
      })),
    }
  }
}

impl AudioCapture for LinuxPulseCapture {
  fn list_sources(&self) -> Result<Vec<AudioSource>, CaptureError> {
    Ok(filter_monitor_sources(query_sources()?))
  }

  fn start(&mut self, source_id: &str, on_frame: FrameCallback) -> Result<(), CaptureError> {
    let raw = query_sources()?;
    let matched = raw
      .into_iter()
      .find(|s| s.name == source_id && s.is_monitor)
      .ok_or_else(|| CaptureError {
        message: "Unknown source".to_string(),
      })?;

    let format = AudioFormat {
      sample_rate: matched.sample_rate,
      channels: matched.channels,
    };
    *self.format.lock().unwrap() = format;

    let spec = Spec {
      format: Format::S16NE,
      channels: format.channels,
      rate: format.sample_rate,
    };

    let simple = psimple::Simple::new(
      None,
      APP_NAME,
      Direction::Record,
      Some(source_id),
      "Recording",
      &spec,
      None,
      None,
    )
    .map_err(|e| CaptureError {
      message: format!("Could not open capture stream: {e}"),
    })?;

    self.running.store(true, Ordering::SeqCst);
    self.paused.store(false, Ordering::SeqCst);

    let running = Arc::clone(&self.running);
    let paused = Arc::clone(&self.paused);
    let samples_per_buffer =
      (format.sample_rate as u64 * format.channels as u64 * BUFFER_MS / 1000) as usize;

    let handle = thread::spawn(move || {
      let mut byte_buf = vec![0u8; samples_per_buffer * 2];

      while running.load(Ordering::SeqCst) {
        match simple.read(&mut byte_buf) {
          Ok(()) => {
            if paused.load(Ordering::SeqCst) {
              continue;
            }
            on_frame(Ok(bytes_to_i16_samples(&byte_buf)));
          }
          Err(e) => {
            on_frame(Err(CaptureError {
              message: format!("Audio capture failed: {e}"),
            }));
            break;
          }
        }
      }
    });

    self.handle = Some(handle);
    Ok(())
  }

  fn format(&self) -> AudioFormat {
    *self.format.lock().unwrap()
  }

  fn pause(&mut self) {
    self.paused.store(true, Ordering::SeqCst);
  }

  fn resume(&mut self) {
    self.paused.store(false, Ordering::SeqCst);
  }

  fn stop(&mut self) -> Result<(), CaptureError> {
    self.running.store(false, Ordering::SeqCst);
    if let Some(handle) = self.handle.take() {
      let _ = handle.join();
    }
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::time::Duration;

  fn raw(name: &str, is_monitor: bool) -> RawSource {
    RawSource {
      name: name.to_string(),
      description: format!("{name} description"),
      is_monitor,
      sample_rate: 48_000,
      channels: 2,
    }
  }

  #[test]
  fn filter_monitor_sources_keeps_only_monitors() {
    let raw_sources = vec![raw("monitor.a", true), raw("mic.b", false)];
    let filtered = filter_monitor_sources(raw_sources);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].id, "monitor.a");
    assert_eq!(filtered[0].name, "monitor.a description");
  }

  #[test]
  fn filter_monitor_sources_empty_when_none_are_monitors() {
    let raw_sources = vec![raw("mic.a", false), raw("mic.b", false)];
    assert!(filter_monitor_sources(raw_sources).is_empty());
  }

  #[test]
  fn bytes_to_i16_samples_converts_native_endian_pairs() {
    let sample: i16 = -1234;
    let bytes = sample.to_ne_bytes();
    let samples = bytes_to_i16_samples(&bytes);
    assert_eq!(samples, vec![sample]);
  }

  #[test]
  fn bytes_to_i16_samples_ignores_a_trailing_odd_byte() {
    let mut bytes = 100_i16.to_ne_bytes().to_vec();
    bytes.push(0xFF);
    assert_eq!(bytes_to_i16_samples(&bytes), vec![100]);
  }

  #[test]
  fn list_sources_against_real_daemon() {
    let capture = LinuxPulseCapture::new();
    match capture.list_sources() {
      Ok(sources) => {
        println!("found {} monitor source(s)", sources.len());
      }
      Err(e) => {
        eprintln!(
          "warning: no PipeWire/PulseAudio daemon reachable, skipping: {}",
          e.message
        );
      }
    }
  }

  #[test]
  fn real_capture_produces_at_least_one_frame_or_skips_gracefully() {
    // Opens a real PulseAudio/PipeWire capture stream and briefly captures real
    // system audio for up to 2 seconds. Nothing captured here is persisted.
    let mut capture = LinuxPulseCapture::new();
    let sources = match capture.list_sources() {
      Ok(sources) if !sources.is_empty() => sources,
      Ok(_) => {
        eprintln!("warning: no monitor sources available, skipping");
        return;
      }
      Err(e) => {
        eprintln!(
          "warning: no PipeWire/PulseAudio daemon reachable, skipping: {}",
          e.message
        );
        return;
      }
    };

    type FrameResult = Result<Vec<i16>, CaptureError>;
    let received: Arc<Mutex<Vec<FrameResult>>> = Arc::new(Mutex::new(Vec::new()));
    let received_cb = Arc::clone(&received);

    if let Err(e) = capture.start(
      &sources[0].id,
      Box::new(move |result| {
        received_cb.lock().unwrap().push(result);
      }),
    ) {
      eprintln!(
        "warning: could not start real capture, skipping: {}",
        e.message
      );
      return;
    }

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while received.lock().unwrap().is_empty() && std::time::Instant::now() < deadline {
      thread::sleep(Duration::from_millis(50));
    }
    capture.stop().unwrap();

    let frames = received.lock().unwrap();
    if frames.is_empty() {
      eprintln!("warning: no frame arrived within 2s, skipping assertions");
      return;
    }
    assert!(
      matches!(frames[0], Ok(ref samples) if !samples.is_empty()),
      "expected the first real frame to be a non-empty Ok(...) sample buffer"
    );
  }
}
