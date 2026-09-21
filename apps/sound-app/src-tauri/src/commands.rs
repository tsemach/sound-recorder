use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, State};

use crate::capture::{AudioFormat, AudioSource, FrameCallback};
use crate::state::{try_transition, CommandError, RecordingState, SharedState};
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
// This is NOT safe against the frame callback (below), which writes
// `RecordingState::Error` from the capture's own background thread — a
// second, genuinely concurrent writer to `state.state` introduced in PR 4.
// A command that already passed its `can_*` guard on a stale `Recording`/
// `Paused` snapshot can still overwrite a just-written `Error` with e.g.
// `Saved`, silently discarding the failure. Low-probability (needs a
// mid-stream capture failure to land in a narrow window against a command
// call) and currently cosmetic, but PR 5 (which will persist `Saved`'s
// `file_path` for real) needs to close this — either the `transition()`
// helper above, or restricting the frame callback's error write to when
// the current state is still `Recording`/`Paused`.
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

  emit_state(&app, &state, RecordingState::Preparing);

  match try_start(&source_id, &state, &app) {
    Ok(source_name) => {
      emit_state(
        &app,
        &state,
        RecordingState::Recording {
          source_name,
          elapsed_ms: 0,
        },
      );
      Ok(())
    }
    Err(e) => {
      emit_state(&app, &state, RecordingState::Idle);
      Err(e)
    }
  }
}

fn try_start(
  source_id: &str,
  state: &SharedState,
  app: &AppHandle,
) -> Result<String, CommandError> {
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

  let dir = match writer::recording_dir(app) {
    Ok(dir) => dir,
    Err(e) => {
      let _ = state.capture.lock().unwrap().stop();
      return Err(CommandError::new(e));
    }
  };
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
  writer_sender: std::sync::mpsc::Sender<WriterMessage>,
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

      let _ = writer_sender.send(WriterMessage::Frame(buffer));
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
  emit_state(
    &app,
    &state,
    RecordingState::Paused {
      source_name,
      elapsed_ms,
    },
  );
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
  emit_state(
    &app,
    &state,
    RecordingState::Recording {
      source_name,
      elapsed_ms,
    },
  );
  Ok(())
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
  if !try_transition(&state.state, RecordingState::can_stop, next.clone()) {
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
        Some(r) => {
          let next = RecordingState::Saved {
            file_path: r.file_path,
            duration_ms: r.duration_ms,
            size_bytes: r.size_bytes,
          };
          *state.state.lock().unwrap() = next.clone();
          let _ = app.emit("recording-state-changed", next);
          Ok(())
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
