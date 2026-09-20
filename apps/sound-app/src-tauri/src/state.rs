#[derive(Debug, Clone, serde::Serialize, PartialEq)]
#[serde(tag = "state")]
pub enum RecordingState {
  Idle,
  Preparing,
  Recording {
    source_name: String,
    elapsed_ms: u64,
  },
  Paused {
    source_name: String,
    elapsed_ms: u64,
  },
  Saving,
  Saved {
    file_path: String,
    duration_ms: u64,
    size_bytes: u64,
  },
  // Not constructed by any production path yet — PR 3's FakeCapture never fails.
  // Real capture (PR 4) and the WAV writer (PR 5) are what actually produce this.
  #[allow(dead_code)]
  Error {
    message: String,
    recoverable: bool,
  },
}

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::capture::AudioCapture;

impl RecordingState {
  pub fn can_start(&self) -> bool {
    matches!(
      self,
      RecordingState::Idle
        | RecordingState::Saved { .. }
        | RecordingState::Error {
          recoverable: true,
          ..
        }
    )
  }

  pub fn can_pause(&self) -> bool {
    matches!(self, RecordingState::Recording { .. })
  }

  pub fn can_resume(&self) -> bool {
    matches!(self, RecordingState::Paused { .. })
  }

  pub fn can_stop(&self) -> bool {
    matches!(
      self,
      RecordingState::Recording { .. } | RecordingState::Paused { .. }
    )
  }

  pub fn can_cancel(&self) -> bool {
    matches!(
      self,
      RecordingState::Recording { .. } | RecordingState::Paused { .. }
    )
  }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandError {
  pub message: String,
  pub recoverable: bool,
}

impl CommandError {
  pub fn new(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
      recoverable: true,
    }
  }
}

pub struct SharedState {
  pub state: Mutex<RecordingState>,
  pub capture: Mutex<Box<dyn AudioCapture>>,
  /// `Arc`-wrapped (unlike `state`/`capture`) because the frame callback passed to
  /// `AudioCapture::start` needs its own cheap, 'static-safe clones of just these
  /// three fields — it can't hold a `tauri::State` guard, which is tied to a single
  /// command invocation's lifetime.
  pub elapsed_ms: Arc<Mutex<u64>>,
  pub level: Arc<Mutex<f32>>,
  pub last_tick_emit: Arc<Mutex<Instant>>,
}

impl SharedState {
  pub fn new(capture: Box<dyn AudioCapture>) -> Self {
    Self {
      state: Mutex::new(RecordingState::Idle),
      capture: Mutex::new(capture),
      elapsed_ms: Arc::new(Mutex::new(0)),
      level: Arc::new(Mutex::new(0.0)),
      last_tick_emit: Arc::new(Mutex::new(Instant::now())),
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn idle_can_start_but_not_pause_stop_cancel() {
    let s = RecordingState::Idle;
    assert!(s.can_start());
    assert!(!s.can_pause());
    assert!(!s.can_resume());
    assert!(!s.can_stop());
    assert!(!s.can_cancel());
  }

  #[test]
  fn recording_can_pause_stop_cancel_but_not_start_or_resume() {
    let s = RecordingState::Recording {
      source_name: "Fake".into(),
      elapsed_ms: 0,
    };
    assert!(!s.can_start());
    assert!(s.can_pause());
    assert!(!s.can_resume());
    assert!(s.can_stop());
    assert!(s.can_cancel());
  }

  #[test]
  fn paused_can_resume_stop_cancel_but_not_start_or_pause() {
    let s = RecordingState::Paused {
      source_name: "Fake".into(),
      elapsed_ms: 1000,
    };
    assert!(!s.can_start());
    assert!(!s.can_pause());
    assert!(s.can_resume());
    assert!(s.can_stop());
    assert!(s.can_cancel());
  }

  #[test]
  fn saved_can_start_again_but_nothing_else() {
    let s = RecordingState::Saved {
      file_path: "fake.wav".into(),
      duration_ms: 1000,
      size_bytes: 0,
    };
    assert!(s.can_start());
    assert!(!s.can_pause());
    assert!(!s.can_resume());
    assert!(!s.can_stop());
    assert!(!s.can_cancel());
  }

  #[test]
  fn recoverable_error_can_start_again_unrecoverable_cannot() {
    let recoverable = RecordingState::Error {
      message: "oops".into(),
      recoverable: true,
    };
    assert!(recoverable.can_start());

    let unrecoverable = RecordingState::Error {
      message: "fatal".into(),
      recoverable: false,
    };
    assert!(!unrecoverable.can_start());
  }

  #[test]
  fn preparing_and_saving_reject_every_action() {
    for s in [RecordingState::Preparing, RecordingState::Saving] {
      assert!(!s.can_start());
      assert!(!s.can_pause());
      assert!(!s.can_resume());
      assert!(!s.can_stop());
      assert!(!s.can_cancel());
    }
  }
}
