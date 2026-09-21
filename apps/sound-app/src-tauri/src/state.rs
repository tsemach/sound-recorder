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
  Error {
    message: String,
    recoverable: bool,
  },
}

use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::capture::{AudioCapture, AudioFormat};

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
  /// `Arc`-wrapped (unlike `capture`) because the frame callback passed to
  /// `AudioCapture::start` needs its own cheap, 'static-safe clone — it can't
  /// hold a `tauri::State` guard, which is tied to a single command
  /// invocation's lifetime. Real capture (PR 4) reports failures from that
  /// background thread by writing `Error` here directly.
  pub state: Arc<Mutex<RecordingState>>,
  pub capture: Mutex<Box<dyn AudioCapture>>,
  pub elapsed_ms: Arc<Mutex<u64>>,
  pub level: Arc<Mutex<f32>>,
  pub last_tick_emit: Arc<Mutex<Instant>>,
  /// Cached copy of the active capture's format, refreshed once right after
  /// a successful `start()`. The frame callback (running on the capture's
  /// own background thread) reads this instead of locking `capture` itself,
  /// since `stop()` holds that lock while joining that same thread —
  /// locking it from inside the callback would deadlock.
  pub format: Arc<Mutex<AudioFormat>>,
  /// The active recording's writer thread handle, if any (PR 5). `stop_recording`/
  /// `cancel_recording` `.take()` this out to send the terminal Finalize/Discard
  /// message and join the thread.
  pub writer: Mutex<Option<crate::writer::WriterHandle>>,
}

impl SharedState {
  pub fn new(capture: Box<dyn AudioCapture>) -> Self {
    Self {
      state: Arc::new(Mutex::new(RecordingState::Idle)),
      capture: Mutex::new(capture),
      elapsed_ms: Arc::new(Mutex::new(0)),
      level: Arc::new(Mutex::new(0.0)),
      last_tick_emit: Arc::new(Mutex::new(Instant::now())),
      format: Arc::new(Mutex::new(AudioFormat {
        sample_rate: 48_000,
        channels: 1,
      })),
      writer: Mutex::new(None),
    }
  }
}

/// Atomically checks and mutates `state` under one lock acquisition, closing
/// the gap where a command could otherwise overwrite a state a concurrent
/// background thread (capture or writer) already moved away from. Returns
/// whether the transition happened.
pub fn try_transition(
  state: &Arc<Mutex<RecordingState>>,
  allowed: impl Fn(&RecordingState) -> bool,
  next: RecordingState,
) -> bool {
  let mut guard = state.lock().unwrap();
  if allowed(&guard) {
    *guard = next;
    true
  } else {
    false
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

  #[test]
  fn try_transition_succeeds_when_allowed() {
    let state = Arc::new(Mutex::new(RecordingState::Recording {
      source_name: "x".into(),
      elapsed_ms: 0,
    }));

    let ok = try_transition(&state, RecordingState::can_stop, RecordingState::Saving);
    assert!(ok);
    assert_eq!(*state.lock().unwrap(), RecordingState::Saving);
  }

  #[test]
  fn try_transition_backs_off_when_state_already_changed() {
    let state = Arc::new(Mutex::new(RecordingState::Recording {
      source_name: "x".into(),
      elapsed_ms: 0,
    }));

    // Simulate a concurrent capture/writer-thread error landing first.
    *state.lock().unwrap() = RecordingState::Error {
      message: "capture failed".into(),
      recoverable: true,
    };

    // A stale stop_recording's guarded transition must NOT clobber this.
    let ok = try_transition(&state, RecordingState::can_stop, RecordingState::Saving);
    assert!(!ok);
    assert_eq!(
      *state.lock().unwrap(),
      RecordingState::Error {
        message: "capture failed".into(),
        recoverable: true
      }
    );
  }
}
