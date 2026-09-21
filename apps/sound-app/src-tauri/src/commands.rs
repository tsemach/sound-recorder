use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, State};

use crate::capture::{AudioFormat, AudioSource, FrameCallback};
use crate::state::{try_transition, CommandError, RecordingState, SharedState};
use crate::storage;
use crate::tick::{buffer_duration_ms, compute_level};
use crate::writer::{self, WriterHandle, WriterMessage};

const TICK_INTERVAL: Duration = Duration::from_millis(100);

// Safe for command-vs-command races only, because every command here is a
// plain sync `fn` (Tauri dispatches these inline on the IPC handler thread,
// never concurrently) — if any command becomes `async` or offloaded to a
// thread pool, the guard-check-then-mutate pattern in every command below
// needs to become a single atomic operation (e.g. a `transition()` helper
// using `std::mem::replace` under one lock acquisition) before that happens.
// See PR 3's final review for detail.
//
// This is NOT safe against the frame callback or the writer thread, both
// of which can write `RecordingState::Error` from their own background
// threads — genuinely concurrent writers to `state.state` introduced in
// PR 4 (capture) and PR 5 (writer). All five commands now guard their
// final state-write with `state::try_transition` (a check-and-set under
// one lock acquisition) instead of calling `emit_state` unguarded — PR 5
// closed this for `stop_recording`/`cancel_recording`, PR 6 (storage
// checks add more Error-producing paths, widening this race's surface)
// extended it to `start_recording`/`pause_recording`/`resume_recording`.
// This function (`emit_state`) now has exactly one remaining unguarded
// caller: `start_recording`'s very first emit, to `Preparing` — safe
// because no capture or writer thread exists yet at that point, so no
// concurrent writer to `state.state` is possible.
fn emit_state(app: &AppHandle, state: &SharedState, next: RecordingState) {
  *state.state.lock().unwrap() = next.clone();
  let _ = app.emit("recording-state-changed", next);
}

#[tauri::command]
pub fn list_sources(state: State<SharedState>) -> Result<Vec<AudioSource>, CommandError> {
  state
    .capture
    .lock()
    .unwrap()
    .list_sources()
    .map_err(|e| CommandError::new(e.message))
}

#[tauri::command]
pub fn start_recording(
  source_id: String,
  state: State<SharedState>,
  app: AppHandle,
) -> Result<(), CommandError> {
  {
    let current = state.state.lock().unwrap();
    if !current.can_start() {
      return Err(CommandError::new(
        "Cannot start recording from the current state",
      ));
    }
  }

  // Safe unguarded: nothing else can be writing to `state.state` yet at
  // this point (no capture or writer thread exists until `try_start` below
  // gets underway), matching the reasoning `stop_recording`'s comment on
  // `emit_state` describes for why an unguarded write is fine when no
  // concurrent writer is possible.
  emit_state(&app, &state, RecordingState::Preparing);

  let still_preparing = |s: &RecordingState| matches!(s, RecordingState::Preparing);

  match try_start(&source_id, &state, &app) {
    Ok(source_name) => {
      let next = RecordingState::Recording {
        source_name,
        elapsed_ms: 0,
      };
      if try_transition(&state.state, still_preparing, next.clone()) {
        let _ = app.emit("recording-state-changed", next);
      }
      Ok(())
    }
    Err(e) => {
      let next = RecordingState::Idle;
      if try_transition(&state.state, still_preparing, next.clone()) {
        let _ = app.emit("recording-state-changed", next);
      }
      Err(e)
    }
  }
}

fn try_start(
  source_id: &str,
  state: &SharedState,
  app: &AppHandle,
) -> Result<String, CommandError> {
  // Defensively tear down any stale capture/writer from a prior recording
  // that never got a clean stop/cancel (e.g. a writer error left the
  // capture thread running with no user-facing way to stop it). Without
  // this, a second start_recording would leave two live capture threads
  // sharing the same running/paused flags (doubling the elapsed-time rate)
  // and silently orphan the old writer handle.
  let _ = state.capture.lock().unwrap().stop();
  state.writer.lock().unwrap().take();

  let dir = writer::recording_dir(app).map_err(CommandError::new)?;
  let free = storage::free_space_bytes(&dir)
    .map_err(|e| CommandError::new(format!("Could not check available disk space: {e}")))?;
  if storage::is_below_threshold(free) {
    return Err(CommandError::new(
      "Not enough free disk space to start recording (need at least 200MB free)",
    ));
  }

  let sources = state
    .capture
    .lock()
    .unwrap()
    .list_sources()
    .map_err(|e| CommandError::new(e.message))?;
  let source_name = sources
    .iter()
    .find(|s| s.id == source_id)
    .map(|s| s.name.clone())
    .ok_or_else(|| CommandError::new("Unknown source"))?;

  *state.elapsed_ms.lock().unwrap() = 0;
  *state.level.lock().unwrap() = 0.0;
  *state.last_tick_emit.lock().unwrap() = Instant::now();

  let (writer_sender, writer_receiver) = writer::create_channel();

  let on_frame = make_frame_callback(
    app.clone(),
    Arc::clone(&state.elapsed_ms),
    Arc::clone(&state.level),
    Arc::clone(&state.last_tick_emit),
    Arc::clone(&state.format),
    Arc::clone(&state.state),
    writer_sender.clone(),
  );

  let mut capture = state.capture.lock().unwrap();
  capture
    .start(source_id, on_frame)
    .map_err(|e| CommandError::new(e.message))?;
  let format = capture.format();
  *state.format.lock().unwrap() = format;
  drop(capture);

  let (temp_path, final_path) = writer::timestamped_wav_paths(&dir);
  let join_handle = match writer::spawn_writer(
    writer_receiver,
    temp_path,
    final_path,
    format,
    app.clone(),
    Arc::clone(&state.state),
  ) {
    Ok(handle) => handle,
    Err(e) => {
      let _ = state.capture.lock().unwrap().stop();
      return Err(CommandError::new(e.message));
    }
  };

  *state.writer.lock().unwrap() = Some(WriterHandle {
    sender: writer_sender,
    join_handle,
  });

  Ok(source_name)
}

/// Builds the callback passed to `AudioCapture::start`. Each PCM buffer updates the
/// shared elapsed/level counters, emits a throttled `recording-tick`, and is forwarded
/// to the writer thread — or, if the capture reports a failure, transitions straight
/// to `RecordingState::Error`.
/// Takes `Arc` clones rather than a `SharedState`/`State` reference because this closure
/// must be `'static` (it runs on the capture's background thread), and `tauri::State` is
/// only valid for the duration of the command invocation that produced it.
fn make_frame_callback(
  app: AppHandle,
  elapsed_ms: Arc<Mutex<u64>>,
  level: Arc<Mutex<f32>>,
  last_tick_emit: Arc<Mutex<Instant>>,
  format: Arc<Mutex<AudioFormat>>,
  state: Arc<Mutex<RecordingState>>,
  writer_sender: std::sync::mpsc::SyncSender<WriterMessage>,
) -> FrameCallback {
  Box::new(
    move |result: Result<Vec<i16>, crate::capture::CaptureError>| {
      let buffer = match result {
        Ok(buffer) => buffer,
        Err(e) => {
          *state.lock().unwrap() = RecordingState::Error {
            message: e.message.clone(),
            recoverable: true,
          };
          let _ = app.emit(
            "recording-state-changed",
            RecordingState::Error {
              message: e.message,
              recoverable: true,
            },
          );
          return;
        }
      };

      let fmt = *format.lock().unwrap();
      let buffer_ms = buffer_duration_ms(buffer.len(), fmt.sample_rate, fmt.channels);
      let mut elapsed_guard = elapsed_ms.lock().unwrap();
      *elapsed_guard += buffer_ms;
      let current_elapsed = *elapsed_guard;
      drop(elapsed_guard);

      let rms = compute_level(&buffer);
      *level.lock().unwrap() = rms;

      let mut last = last_tick_emit.lock().unwrap();
      if last.elapsed() >= TICK_INTERVAL {
        *last = Instant::now();
        drop(last);
        let _ = app.emit(
          "recording-tick",
          serde_json::json!({ "elapsed_ms": current_elapsed, "level": rms }),
        );
      }

      match writer_sender.try_send(WriterMessage::Frame(buffer)) {
        Ok(()) => {}
        Err(std::sync::mpsc::TrySendError::Full(_)) => {
          // The writer can't keep up. Drop this one frame rather than block
          // the capture thread (a blocking send here would reintroduce the
          // exact capture/disk coupling PR 4's review eliminated) -- but
          // treat a full channel as a real failure signal, same as a
          // disk-space or write error.
          let message = "Recording stopped: the audio writer fell behind".to_string();
          *state.lock().unwrap() = RecordingState::Error {
            message: message.clone(),
            recoverable: true,
          };
          let _ = app.emit(
            "recording-state-changed",
            RecordingState::Error {
              message,
              recoverable: true,
            },
          );
        }
        Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
          // Writer thread already exited (e.g. its own error path) --
          // nothing to do, the state was already set by whatever caused
          // that exit.
        }
      }
    },
  )
}

#[tauri::command]
pub fn pause_recording(state: State<SharedState>, app: AppHandle) -> Result<(), CommandError> {
  let source_name = {
    let current = state.state.lock().unwrap();
    if !current.can_pause() {
      return Err(CommandError::new("Cannot pause unless recording"));
    }
    match &*current {
      RecordingState::Recording { source_name, .. } => source_name.clone(),
      _ => unreachable!(),
    }
  };

  state.capture.lock().unwrap().pause();
  let elapsed_ms = *state.elapsed_ms.lock().unwrap();
  let next = RecordingState::Paused {
    source_name,
    elapsed_ms,
  };
  if try_transition(&state.state, RecordingState::can_pause, next.clone()) {
    let _ = app.emit("recording-state-changed", next);
  }
  Ok(())
}

#[tauri::command]
pub fn resume_recording(state: State<SharedState>, app: AppHandle) -> Result<(), CommandError> {
  let source_name = {
    let current = state.state.lock().unwrap();
    if !current.can_resume() {
      return Err(CommandError::new("Cannot resume unless paused"));
    }
    match &*current {
      RecordingState::Paused { source_name, .. } => source_name.clone(),
      _ => unreachable!(),
    }
  };

  state.capture.lock().unwrap().resume();
  let elapsed_ms = *state.elapsed_ms.lock().unwrap();
  let next = RecordingState::Recording {
    source_name,
    elapsed_ms,
  };
  if try_transition(&state.state, RecordingState::can_resume, next.clone()) {
    let _ = app.emit("recording-state-changed", next);
  }
  Ok(())
}

/// The guarded `Recording`/`Paused` -> `Saving` transition `stop_recording`
/// performs right after stopping capture and taking the writer handle.
/// Extracted into its own function so it's directly unit-testable without a
/// real `AppHandle` (see this file's `tests` module for why `stop_recording`
/// itself can't be called from a plain unit test) — mirrors the pattern
/// `writer.rs`'s own tests use for `run_writer_loop`: test the extracted
/// decision logic rather than fight to construct Tauri runtime scaffolding
/// a unit test doesn't have.
fn try_begin_saving(state: &Arc<Mutex<RecordingState>>) -> bool {
  try_transition(state, RecordingState::can_stop, RecordingState::Saving)
}

#[tauri::command]
pub fn stop_recording(state: State<SharedState>, app: AppHandle) -> Result<(), CommandError> {
  {
    let current = state.state.lock().unwrap();
    if !current.can_stop() {
      return Err(CommandError::new("Cannot stop unless recording or paused"));
    }
  }

  // Stop capture FIRST: `.stop()` joins the capture thread, so if it failed
  // concurrently, its `Error` state-write is guaranteed to have already
  // landed by the time this returns (thread join is a happens-before edge).
  let capture_result = state.capture.lock().unwrap().stop();
  let writer_handle = state.writer.lock().unwrap().take();

  let next = RecordingState::Saving;
  if !try_begin_saving(&state.state) {
    // The state already moved on (almost certainly a concurrent capture or
    // writer Error) -- don't clobber it, and don't discard the writer
    // either. Just drop `writer_handle` here: dropping its `sender` closes
    // the channel, so the writer thread's `recv()` returns an error and it
    // exits gracefully on its own, leaving the (already flushed, so already
    // valid up to the last frame) temp file in place for
    // `recovery::recover_orphaned_recordings` to promote or delete at next
    // startup -- consistent with this PR's "preserve a partial recording
    // rather than silently discard it" goal. Dropping the `JoinHandle`
    // without joining does not block: the thread keeps running detached.
    return Ok(());
  }
  let _ = app.emit("recording-state-changed", next);

  match capture_result {
    Ok(()) => {
      let result = writer_handle.and_then(|handle| {
        let _ = handle.sender.send(WriterMessage::Finalize);
        handle.join_handle.join().ok().flatten()
      });
      match result {
        Some(r) if r.size_bytes > 44 => {
          let next = RecordingState::Saved {
            file_path: r.file_path,
            duration_ms: r.duration_ms,
            size_bytes: r.size_bytes,
          };
          *state.state.lock().unwrap() = next.clone();
          let _ = app.emit("recording-state-changed", next);
          Ok(())
        }
        Some(_) => {
          // A header-only result (no real audio data) means either an
          // instant record-then-stop, or a race where a writer/capture
          // error happened but got clobbered before the guard could catch
          // it. Report it as a recoverable error rather than a misleading
          // "Saved" with an effectively empty file.
          let message = "No audio was captured before the recording stopped".to_string();
          let next = RecordingState::Error {
            message: message.clone(),
            recoverable: true,
          };
          *state.state.lock().unwrap() = next.clone();
          let _ = app.emit("recording-state-changed", next);
          Err(CommandError::new(message))
        }
        None => {
          let message = "Recording stopped, but the audio file could not be saved".to_string();
          let next = RecordingState::Error {
            message: message.clone(),
            recoverable: true,
          };
          *state.state.lock().unwrap() = next.clone();
          let _ = app.emit("recording-state-changed", next);
          Err(CommandError::new(message))
        }
      }
    }
    Err(e) => {
      // Same rationale as the race-backoff branch above: don't discard,
      // just drop the handle so the writer's already-flushed temp file
      // survives for recovery instead of being actively deleted.
      drop(writer_handle);
      let next = RecordingState::Error {
        message: e.message.clone(),
        recoverable: true,
      };
      *state.state.lock().unwrap() = next.clone();
      let _ = app.emit("recording-state-changed", next);
      Err(CommandError::new(e.message))
    }
  }
}

#[tauri::command]
pub fn cancel_recording(state: State<SharedState>, app: AppHandle) -> Result<(), CommandError> {
  {
    let current = state.state.lock().unwrap();
    if !current.can_cancel() {
      return Err(CommandError::new(
        "Cannot cancel unless recording or paused",
      ));
    }
  }

  let capture_result = state.capture.lock().unwrap().stop();
  let writer_handle = state.writer.lock().unwrap().take();

  if let Some(handle) = writer_handle {
    let _ = handle.sender.send(WriterMessage::Discard);
    let _ = handle.join_handle.join();
  }

  capture_result.map_err(|e| CommandError::new(e.message))?;

  let next = RecordingState::Idle;
  if try_transition(&state.state, RecordingState::can_cancel, next.clone()) {
    let _ = app.emit("recording-state-changed", next);
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::capture::fake::FakeCapture;

  // `stop_recording` itself is a `#[tauri::command]` taking a real
  // `tauri::AppHandle`/`State<SharedState>`, both concretely tied to the
  // `Wry` runtime (`pub type AppHandle<R = crate::Wry> = ...` in tauri
  // 2.11.6's own source). `tauri::test::mock_builder()` builds an
  // `App`/`AppHandle` over `MockRuntime` instead, which does not unify
  // with `Wry` -- so it can't be substituted into these command signatures
  // without making `commands.rs` generic over `Runtime`, a much larger
  // change than this fix warrants. So this test exercises
  // `try_begin_saving`, the exact guarded-transition call `stop_recording`
  // performs, directly against a real `SharedState` built the same way
  // this crate's other module-level tests do (via `FakeCapture`) --
  // mirroring the pattern `writer.rs`'s own tests use for
  // `run_writer_loop` of testing extracted decision logic instead of
  // fighting Tauri runtime scaffolding in a unit test.
  #[test]
  fn stop_recording_guard_does_not_clobber_a_concurrent_error() {
    let state = SharedState::new(Box::new(FakeCapture::new()));
    *state.state.lock().unwrap() = RecordingState::Recording {
      source_name: "test".into(),
      elapsed_ms: 0,
    };

    // Simulate a concurrent capture/writer error landing first, exactly as
    // stop_recording's own race-backoff comment describes.
    *state.state.lock().unwrap() = RecordingState::Error {
      message: "simulated".into(),
      recoverable: true,
    };

    let transitioned = try_begin_saving(&state.state);

    assert!(
      !transitioned,
      "the guarded transition must back off, not clobber the race"
    );
    assert_eq!(
      *state.state.lock().unwrap(),
      RecordingState::Error {
        message: "simulated".into(),
        recoverable: true,
      },
      "state must still be the concurrently-written Error, not Saving"
    );
  }
}
