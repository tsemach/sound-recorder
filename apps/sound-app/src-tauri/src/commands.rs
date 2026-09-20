use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, State};

use crate::capture::{AudioSource, FrameCallback};
use crate::state::{CommandError, RecordingState, SharedState};
use crate::tick::{buffer_duration_ms, compute_level};

const TICK_INTERVAL: Duration = Duration::from_millis(100);

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

  let on_frame = make_frame_callback(
    app.clone(),
    Arc::clone(&state.elapsed_ms),
    Arc::clone(&state.level),
    Arc::clone(&state.last_tick_emit),
  );

  state
    .capture
    .lock()
    .unwrap()
    .start(&source_id, on_frame)
    .map_err(|e| CommandError::new(e.message))?;

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

/// Builds the callback passed to `AudioCapture::start`. Each fake (or, later, real) PCM
/// buffer updates the shared elapsed/level counters and emits a throttled `recording-tick`.
/// Takes `Arc` clones rather than a `SharedState`/`State` reference because this closure
/// must be `'static` (it runs on the capture's background thread), and `tauri::State` is
/// only valid for the duration of the command invocation that produced it.
fn make_frame_callback(
  app: AppHandle,
  elapsed_ms: Arc<Mutex<u64>>,
  level: Arc<Mutex<f32>>,
  last_tick_emit: Arc<Mutex<Instant>>,
) -> FrameCallback {
  Box::new(move |buffer: Vec<i16>| {
    let buffer_ms = buffer_duration_ms(buffer.len());
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
  })
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

  emit_state(&app, &state, RecordingState::Saving);

  state
    .capture
    .lock()
    .unwrap()
    .stop()
    .map_err(|e| CommandError::new(e.message))?;

  let duration_ms = *state.elapsed_ms.lock().unwrap();
  emit_state(
    &app,
    &state,
    RecordingState::Saved {
      file_path: "fake-recording.wav".to_string(),
      duration_ms,
      size_bytes: 0,
    },
  );

  Ok(())
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

  state
    .capture
    .lock()
    .unwrap()
    .stop()
    .map_err(|e| CommandError::new(e.message))?;

  emit_state(&app, &state, RecordingState::Idle);
  Ok(())
}
