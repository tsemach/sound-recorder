use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::{AudioCapture, AudioSource, CaptureError, FrameCallback};

#[allow(dead_code)] // Unused until Task 3 wires AudioCapture into commands.rs
const SAMPLE_RATE: f32 = 48_000.0;
#[allow(dead_code)] // Unused until Task 3 wires AudioCapture into commands.rs
const BUFFER_MS: u64 = 20;

#[allow(dead_code)] // Unused until Task 3 wires AudioCapture into commands.rs
pub struct FakeCapture {
  running: Arc<AtomicBool>,
  paused: Arc<AtomicBool>,
  handle: Option<thread::JoinHandle<()>>,
}

impl FakeCapture {
  #[allow(dead_code)] // Unused until Task 3 wires AudioCapture into commands.rs
  pub fn new() -> Self {
    Self {
      running: Arc::new(AtomicBool::new(false)),
      paused: Arc::new(AtomicBool::new(false)),
      handle: None,
    }
  }
}

impl AudioCapture for FakeCapture {
  fn list_sources(&self) -> Result<Vec<AudioSource>, CaptureError> {
    Ok(vec![
      AudioSource {
        id: "fake-system-audio".to_string(),
        name: "Fake System Audio".to_string(),
      },
      AudioSource {
        id: "fake-microphone".to_string(),
        name: "Fake Microphone".to_string(),
      },
    ])
  }

  fn start(&mut self, _source_id: &str, on_frame: FrameCallback) -> Result<(), CaptureError> {
    self.running.store(true, Ordering::SeqCst);
    self.paused.store(false, Ordering::SeqCst);

    let running = Arc::clone(&self.running);
    let paused = Arc::clone(&self.paused);

    let handle = thread::spawn(move || {
      let samples_per_buffer = (SAMPLE_RATE as u64 * BUFFER_MS / 1000) as usize;
      let frequency = 440.0_f32;
      let mut phase = 0.0_f32;

      while running.load(Ordering::SeqCst) {
        if paused.load(Ordering::SeqCst) {
          thread::sleep(Duration::from_millis(BUFFER_MS));
          continue;
        }

        let mut buffer = Vec::with_capacity(samples_per_buffer);
        for _ in 0..samples_per_buffer {
          let sample = (phase.sin() * i16::MAX as f32 * 0.2) as i16;
          buffer.push(sample);
          phase += 2.0 * std::f32::consts::PI * frequency / SAMPLE_RATE;
        }

        on_frame(buffer);
        thread::sleep(Duration::from_millis(BUFFER_MS));
      }
    });

    self.handle = Some(handle);
    Ok(())
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
  use std::sync::{Arc, Mutex};
  use std::thread;
  use std::time::Duration;

  #[test]
  fn produces_frames_while_running() {
    let mut capture = FakeCapture::new();
    let received: Arc<Mutex<Vec<Vec<i16>>>> = Arc::new(Mutex::new(Vec::new()));
    let received_cb = Arc::clone(&received);

    capture
      .start(
        "fake-system-audio",
        Box::new(move |buffer| {
          received_cb.lock().unwrap().push(buffer);
        }),
      )
      .unwrap();

    thread::sleep(Duration::from_millis(100));
    capture.stop().unwrap();

    let frames = received.lock().unwrap();
    assert!(
      !frames.is_empty(),
      "expected at least one fake frame to be produced"
    );
    assert!(!frames[0].is_empty(), "frame should contain samples");
  }

  #[test]
  fn pause_stops_producing_frames() {
    let mut capture = FakeCapture::new();
    let received: Arc<Mutex<Vec<Vec<i16>>>> = Arc::new(Mutex::new(Vec::new()));
    let received_cb = Arc::clone(&received);

    capture
      .start(
        "fake-system-audio",
        Box::new(move |buffer| {
          received_cb.lock().unwrap().push(buffer);
        }),
      )
      .unwrap();
    thread::sleep(Duration::from_millis(50));
    capture.pause();

    let count_at_pause = received.lock().unwrap().len();
    thread::sleep(Duration::from_millis(100));
    let count_after_pause = received.lock().unwrap().len();

    capture.stop().unwrap();

    assert_eq!(
      count_at_pause, count_after_pause,
      "no new frames should arrive while paused"
    );
  }

  #[test]
  fn list_sources_returns_fake_entries() {
    let capture = FakeCapture::new();
    let sources = capture.list_sources().unwrap();
    assert_eq!(sources.len(), 2);
    assert!(sources.iter().any(|s| s.id == "fake-system-audio"));
  }
}
