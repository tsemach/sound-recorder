# sound-app: Recording State Machine + Mocked Capture (PR 3) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace PR 2's throwaway `ping` scaffolding with a real Rust recording state machine, a full command/event surface, and real frontend UI — backed by a fake (not real audio yet) capture source — so a user can click through Record → Pause → Resume → Stop and watch the whole system work.

**Architecture:** `AudioCapture` trait + `FakeCapture` (a background thread generating a synthetic sine-wave PCM stream) live under `capture/`. `RecordingState` + transition-legality predicates live in `state.rs`. Commands in `commands.rs` validate transitions, drive `FakeCapture`, and emit `recording-state-changed`/`recording-tick` events. The frontend's `useRecordingState` hook wraps both events plus the five commands, and `App.tsx` renders purely from that hook's state.

**Tech Stack:** Rust (existing Tauri v2 backend), React 19 + TypeScript (existing Vite frontend), Vitest, `@tauri-apps/api` (`core` + `event` modules).

**Spec:** `docs/superpowers/specs/2026-09-20-sound-app-recording-state-machine-design.md`

## Global Constraints

- Command surface is recording-control only: `list_sources`, `start_recording`, `pause_recording`, `resume_recording`, `stop_recording`, `cancel_recording`. `list_recordings`/`delete_recording`/`rename_recording` are explicitly out of scope (PR 7).
- Every command returns `Result<T, CommandError>` (`{ message: String, recoverable: bool }`) — illegal transitions are rejected, never silently ignored.
- The `ping` command and its frontend button (PR 2) are removed entirely.
- `tauri.conf.json`'s CSP becomes `"default-src 'self'; style-src 'self' 'unsafe-inline'"` — **verified working** during planning (live `tauri dev` run, full click-through, confirmed by direct visual check: source dropdown populated, Record/Pause/Resume/Stop/Cancel all worked, elapsed timer counted up). A one-time benign console warning ("IPC custom protocol failed, Tauri will now use the postMessage interface instead") appears on startup under this CSP — this is Tauri's own documented graceful fallback transport, not a functional problem; do not attempt to "fix" it.
- No new shadcn components — source selector is a plain `<select>`, the level meter is a plain `<div>` bar, cancel confirmation uses native `window.confirm()`.
- Rust code must pass `cargo fmt --check` (2-space, per `src-tauri/rustfmt.toml` from PR 2) and `cargo clippy --all-targets` with zero warnings.
- Frontend code must pass `pnpm lint` (ESLint, including the `react-hooks` plugin — do not derive default/initial state via a `useEffect` + `setState`; compute derived values at render time instead) and `pnpm typecheck`.

---

## Task 1: `AudioCapture` trait + `FakeCapture`

**Files:**
- Create: `apps/sound-app/src-tauri/src/capture/mod.rs`
- Create: `apps/sound-app/src-tauri/src/capture/fake.rs`
- Modify: `apps/sound-app/src-tauri/src/lib.rs` (add `mod capture;` only — do not touch anything else in this file yet)

**Interfaces:**
- Produces: `trait AudioCapture: Send { fn list_sources(&self) -> Result<Vec<AudioSource>, CaptureError>; fn start(&mut self, source_id: &str, on_frame: FrameCallback) -> Result<(), CaptureError>; fn pause(&mut self); fn resume(&mut self); fn stop(&mut self) -> Result<(), CaptureError>; }`, `struct AudioSource { id: String, name: String }` (Clone, Serialize), `struct CaptureError { message: String }`, `type FrameCallback = Box<dyn Fn(Vec<i16>) + Send + 'static>`, and `struct FakeCapture` implementing the trait. Task 2 and Task 3 depend on these exact names/signatures.

- [ ] **Step 1: Write the failing tests**

Create `apps/sound-app/src-tauri/src/capture/mod.rs`:

```rust
pub mod fake;

#[derive(Clone, serde::Serialize)]
pub struct AudioSource {
  pub id: String,
  pub name: String,
}

#[derive(Debug, Clone)]
pub struct CaptureError {
  pub message: String,
}

pub type FrameCallback = Box<dyn Fn(Vec<i16>) + Send + 'static>;

pub trait AudioCapture: Send {
  fn list_sources(&self) -> Result<Vec<AudioSource>, CaptureError>;
  fn start(&mut self, source_id: &str, on_frame: FrameCallback) -> Result<(), CaptureError>;
  fn pause(&mut self);
  fn resume(&mut self);
  fn stop(&mut self) -> Result<(), CaptureError>;
}
```

Create `apps/sound-app/src-tauri/src/capture/fake.rs` with only the test module for now (the `FakeCapture` struct doesn't exist yet, so this fails to compile):

```rust
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
```

Add `mod capture;` as the first line of `apps/sound-app/src-tauri/src/lib.rs` (leave everything else in that file untouched for now).

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd apps/sound-app/src-tauri
cargo test capture::fake
```
Expected: FAIL to compile — `FakeCapture` is not defined (`AudioCapture` trait import via `use super::*` also unresolved since `fake.rs` has no `use` statement for it yet — that's fine, the compile error itself is the RED signal).

- [ ] **Step 3: Implement `FakeCapture`**

Replace `apps/sound-app/src-tauri/src/capture/fake.rs` with:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use super::{AudioCapture, AudioSource, CaptureError, FrameCallback};

const SAMPLE_RATE: f32 = 48_000.0;
const BUFFER_MS: u64 = 20;

pub struct FakeCapture {
  running: Arc<AtomicBool>,
  paused: Arc<AtomicBool>,
  handle: Option<thread::JoinHandle<()>>,
}

impl FakeCapture {
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
```

Then re-add the same `#[cfg(test)] mod tests { ... }` block from Step 1 at the end of this file (it was already written; just make sure it's present after replacing the file).

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd apps/sound-app/src-tauri
cargo test capture::fake
```
Expected: PASS — 3 tests (`produces_frames_while_running`, `pause_stops_producing_frames`, `list_sources_returns_fake_entries`).

- [ ] **Step 5: Verify formatting and lints**

```bash
cd apps/sound-app/src-tauri
cargo fmt
cargo fmt --check
cargo clippy --all-targets
```
Expected: `cargo fmt --check` exits 0 (no reformatting needed after `cargo fmt` runs once), `cargo clippy` produces zero warnings.

- [ ] **Step 6: Commit**

```bash
cd /home/tsemach/projects/sound-recorder
git add apps/sound-app/src-tauri/src/capture apps/sound-app/src-tauri/src/lib.rs
git commit -m "feat(sound-app): add AudioCapture trait and FakeCapture"
```

---

## Task 2: `RecordingState` + `SharedState` + transition predicates

**Files:**
- Create: `apps/sound-app/src-tauri/src/state.rs`
- Modify: `apps/sound-app/src-tauri/src/lib.rs` (add `mod state;` only)

**Interfaces:**
- Consumes: `capture::AudioCapture` trait (Task 1).
- Produces: `enum RecordingState` (7 variants: `Idle`, `Preparing`, `Recording { source_name, elapsed_ms }`, `Paused { source_name, elapsed_ms }`, `Saving`, `Saved { file_path, duration_ms, size_bytes }`, `Error { message, recoverable }`), its 5 predicate methods (`can_start`/`can_pause`/`can_resume`/`can_stop`/`can_cancel`), `struct CommandError { message, recoverable }` with `CommandError::new(impl Into<String>)`, and `struct SharedState` with fields `state: Mutex<RecordingState>`, `capture: Mutex<Box<dyn AudioCapture>>`, `elapsed_ms: Arc<Mutex<u64>>`, `level: Arc<Mutex<f32>>`, `last_tick_emit: Arc<Mutex<Instant>>`, plus `SharedState::new(capture: Box<dyn AudioCapture>)`. Task 3 depends on every one of these exact names.

- [ ] **Step 1: Write the failing tests**

Create `apps/sound-app/src-tauri/src/state.rs` with only the enum, predicates, and test module (no `CommandError`/`SharedState` yet — those get added in Step 3, since the tests only need `RecordingState`):

```rust
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
```

Add `mod state;` to `apps/sound-app/src-tauri/src/lib.rs` (below `mod capture;`).

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd apps/sound-app/src-tauri
cargo test state::tests
```
Expected: FAIL to compile — `can_start`/`can_pause`/etc. are not methods on `RecordingState` yet.

- [ ] **Step 3: Implement the predicates, `CommandError`, and `SharedState`**

Add this to `apps/sound-app/src-tauri/src/state.rs`, right after the `RecordingState` enum definition and before the `#[cfg(test)]` module:

```rust
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
```

Also change the enum's derive line from `#[derive(Debug, Clone, serde::Serialize, PartialEq)]` to keep it as-is (already correct) — no change needed there, this step only adds the new code below the enum.

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd apps/sound-app/src-tauri
cargo test state::tests
```
Expected: PASS — 6 tests.

- [ ] **Step 5: Verify formatting and lints**

```bash
cd apps/sound-app/src-tauri
cargo fmt
cargo fmt --check
cargo clippy --all-targets
```
Expected: both clean. (The `#[allow(dead_code)]` on the `Error` variant is required here — without it, `cargo clippy` reports `variant is never constructed` since nothing in this PR's production code paths creates one.)

- [ ] **Step 6: Commit**

```bash
cd /home/tsemach/projects/sound-recorder
git add apps/sound-app/src-tauri/src/state.rs apps/sound-app/src-tauri/src/lib.rs
git commit -m "feat(sound-app): add RecordingState machine and SharedState"
```

---

## Task 3: Tick computation + commands + final `lib.rs` wiring

**Files:**
- Create: `apps/sound-app/src-tauri/src/tick.rs`
- Create: `apps/sound-app/src-tauri/src/commands.rs`
- Modify: `apps/sound-app/src-tauri/src/lib.rs` (full rewrite — removes `ping`, adds `mod tick;`/`mod commands;`, registers the 6 new commands, calls `.manage(...)`)

**Interfaces:**
- Consumes: everything from Task 1 (`AudioCapture`, `FakeCapture`, `FrameCallback`, `AudioSource`) and Task 2 (`RecordingState`, `CommandError`, `SharedState`).
- Produces: `#[tauri::command]` functions `list_sources`, `start_recording(source_id: String)`, `pause_recording`, `resume_recording`, `stop_recording`, `cancel_recording` — Task 5 (frontend hook) depends on these exact command names and, for `start_recording`, the exact camelCase JS-side argument name `sourceId` (Tauri auto-converts the Rust `source_id` parameter). Also produces the two events `recording-state-changed` (payload: the full serialized `RecordingState`) and `recording-tick` (payload: `{ elapsed_ms: number, level: number }`) that Task 5 listens for.

- [ ] **Step 1: Write the failing tests**

Create `apps/sound-app/src-tauri/src/tick.rs`:

```rust
pub const SAMPLE_RATE_HZ: u64 = 48_000;

/// Duration in milliseconds represented by `sample_count` mono samples at `SAMPLE_RATE_HZ`.
pub fn buffer_duration_ms(sample_count: usize) -> u64 {
  (sample_count as u64 * 1000) / SAMPLE_RATE_HZ
}

/// Root-mean-square level of a PCM buffer, normalized to 0.0..=1.0.
pub fn compute_level(samples: &[i16]) -> f32 {
  if samples.is_empty() {
    return 0.0;
  }
  let sum_squares: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
  let mean_square = sum_squares / samples.len() as f64;
  let rms = mean_square.sqrt();
  (rms / i16::MAX as f64) as f32
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn buffer_duration_matches_sample_rate() {
    // 48 samples at 48kHz = 1ms
    assert_eq!(buffer_duration_ms(48), 1);
    // 960 samples at 48kHz = 20ms
    assert_eq!(buffer_duration_ms(960), 20);
  }

  #[test]
  fn silence_has_zero_level() {
    let silence = vec![0_i16; 960];
    assert_eq!(compute_level(&silence), 0.0);
  }

  #[test]
  fn full_scale_square_wave_has_level_near_one() {
    let loud = vec![i16::MAX; 960];
    let level = compute_level(&loud);
    assert!(
      level > 0.99 && level <= 1.0,
      "expected near-1.0, got {level}"
    );
  }

  #[test]
  fn empty_buffer_has_zero_level() {
    assert_eq!(compute_level(&[]), 0.0);
  }
}
```

This file is self-contained and compiles/passes immediately (no separate RED step needed for it — it has no dependency on not-yet-written code). Run it to confirm:

```bash
cd apps/sound-app/src-tauri
cargo test tick::tests
```
Expected: PASS — 4 tests, immediately (this module has nothing to be RED against).

- [ ] **Step 2: Implement `commands.rs`**

Create `apps/sound-app/src-tauri/src/commands.rs`:

```rust
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
```

There are no unit tests directly on these `#[tauri::command]` functions in this task — they're thin orchestration over `state.rs` (already tested in Task 2) and `tick.rs` (already tested above). Their correctness is verified by Task 7's manual end-to-end click-through, which is the appropriate level for Tauri command wrappers (mocking a full `tauri::State`/`AppHandle` for unit tests here would test the mock, not real behavior).

- [ ] **Step 3: Rewrite `lib.rs`**

Replace the entire contents of `apps/sound-app/src-tauri/src/lib.rs` with:

```rust
mod capture;
mod commands;
mod state;
mod tick;

use capture::fake::FakeCapture;
use state::SharedState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      Ok(())
    })
    .manage(SharedState::new(Box::new(FakeCapture::new())))
    .invoke_handler(tauri::generate_handler![
      commands::list_sources,
      commands::start_recording,
      commands::pause_recording,
      commands::resume_recording,
      commands::stop_recording,
      commands::cancel_recording,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
```

This removes the `ping` command and its test module entirely — they were PR 2's explicit throwaway scaffolding.

- [ ] **Step 4: Verify everything compiles and all tests pass**

```bash
cd apps/sound-app/src-tauri
cargo test
```
Expected: PASS — 13 tests total (3 from `capture::fake`, 6 from `state`, 4 from `tick`), 0 failures. No `ping_returns_pong` test anymore (deleted with `lib.rs`'s rewrite).

- [ ] **Step 5: Verify formatting, lints, and a real build**

```bash
cd apps/sound-app/src-tauri
cargo fmt
cargo fmt --check
cargo clippy --all-targets
cargo check
```
Expected: all clean, zero warnings.

- [ ] **Step 6: Commit**

```bash
cd /home/tsemach/projects/sound-recorder
git add apps/sound-app/src-tauri/src/tick.rs apps/sound-app/src-tauri/src/commands.rs apps/sound-app/src-tauri/src/lib.rs
git commit -m "feat(sound-app): wire recording commands and events, remove ping"
```

---

## Task 4: CSP fix + `test:rust` script wiring

**Files:**
- Modify: `apps/sound-app/src-tauri/tauri.conf.json`
- Modify: `apps/sound-app/package.json`
- Modify: `package.json` (root)
- Modify: `turbo.json`

**Interfaces:** none — this is pure configuration, no code interfaces produced or consumed.

- [ ] **Step 1: Set the real CSP**

In `apps/sound-app/src-tauri/tauri.conf.json`, change:
```json
    "security": {
      "csp": null
    }
```
to:
```json
    "security": {
      "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'"
    }
```
This exact value was verified working end-to-end during planning (full click-through: source dropdown, Record/Pause/Resume/Stop/Cancel, elapsed timer). A benign one-time console warning ("IPC custom protocol failed, Tauri will now use the postMessage interface instead") is expected and is not a bug — see Global Constraints.

- [ ] **Step 2: Add the `test:rust` scripts**

In `apps/sound-app/package.json`, add to `scripts` (alongside the existing `dev`/`build`/`preview`/`lint`/`format`/`test`/`typecheck`):
```json
"test:rust": "cd src-tauri && cargo test"
```

In root `package.json`, add to `scripts` (alongside the existing `build`/`dev`/`lint`/`format`/`typecheck`/`test`):
```json
"test:rust": "turbo test:rust"
```

In `turbo.json`, add a `test:rust` task matching the existing task shapes (alongside `build`/`lint`/`format`/`typecheck`/`test`/`dev`):
```json
    "test:rust": {
      "dependsOn": ["^test:rust"]
    },
```

- [ ] **Step 3: Verify**

```bash
cd /home/tsemach/projects/sound-recorder
pnpm test:rust
```
Expected: exits 0, runs the 13 Rust tests via Turbo (only `sound-app` defines this script, matching how the existing `test` task only runs for `sound-app`).

```bash
pnpm test
```
Expected: still exits 0, still Vitest-only (unaffected by this change — confirms `test:rust` stayed separate as required).

- [ ] **Step 4: Commit**

```bash
cd /home/tsemach/projects/sound-recorder
git add apps/sound-app/src-tauri/tauri.conf.json apps/sound-app/package.json package.json turbo.json
git commit -m "fix(sound-app): set restrictive CSP, add separate cargo test script"
```

---

## Task 5: `useRecordingState` hook (TDD)

**Files:**
- Create: `apps/sound-app/src/hooks/useRecordingState.ts`
- Test: `apps/sound-app/src/hooks/useRecordingState.test.ts`

**Interfaces:**
- Consumes: `invoke` from `@tauri-apps/api/core`, `listen` from `@tauri-apps/api/event`; the command names and event names from Task 3 (`list_sources`, `start_recording` with JS arg `sourceId`, `pause_recording`, `resume_recording`, `stop_recording`, `cancel_recording`, events `recording-state-changed`/`recording-tick`).
- Produces: `export type AudioSource = { id: string; name: string }`, `export type RecordingState` (discriminated union matching the Rust enum's `#[serde(tag = "state")]` JSON shape exactly: `{ state: "Idle" } | { state: "Preparing" } | { state: "Recording"; source_name: string; elapsed_ms: number } | { state: "Paused"; source_name: string; elapsed_ms: number } | { state: "Saving" } | { state: "Saved"; file_path: string; duration_ms: number; size_bytes: number } | { state: "Error"; message: string; recoverable: boolean }`), and `export function useRecordingState()` returning `{ state, elapsedMs, level, sources, error, startRecording, pauseRecording, resumeRecording, stopRecording, cancelRecording }`. Task 6 depends on every one of these exact names and the `RecordingState` variant shapes.

- [ ] **Step 1: Write the failing tests**

Create `apps/sound-app/src/hooks/useRecordingState.test.ts`:

```ts
import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

const { listeners, mockInvoke } = vi.hoisted(() => ({
  listeners: {} as Record<string, (event: { payload: unknown }) => void>,
  mockInvoke: vi.fn(),
}))

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}))

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    (
      event: string,
      callback: (event: { payload: unknown }) => void
    ) => {
      listeners[event] = callback
      return Promise.resolve(() => {
        delete listeners[event]
      })
    }
  ),
}))

import { useRecordingState } from "./useRecordingState"

describe("useRecordingState", () => {
  beforeEach(() => {
    mockInvoke.mockReset()
    mockInvoke.mockResolvedValue([
      { id: "fake-system-audio", name: "Fake System Audio" },
    ])
  })

  it("loads sources on mount", async () => {
    const { result } = renderHook(() => useRecordingState())
    await waitFor(() => expect(result.current.sources).toHaveLength(1))
    expect(mockInvoke).toHaveBeenCalledWith("list_sources")
  })

  it("updates state when a recording-state-changed event arrives", async () => {
    const { result } = renderHook(() => useRecordingState())
    await waitFor(() =>
      expect(listeners["recording-state-changed"]).toBeDefined()
    )

    act(() => {
      listeners["recording-state-changed"]!({
        payload: {
          state: "Recording",
          source_name: "Fake System Audio",
          elapsed_ms: 0,
        },
      })
    })

    expect(result.current.state).toEqual({
      state: "Recording",
      source_name: "Fake System Audio",
      elapsed_ms: 0,
    })
  })

  it("updates elapsedMs and level when a recording-tick event arrives", async () => {
    const { result } = renderHook(() => useRecordingState())
    await waitFor(() => expect(listeners["recording-tick"]).toBeDefined())

    act(() => {
      listeners["recording-tick"]!({
        payload: { elapsed_ms: 4200, level: 0.5 },
      })
    })

    expect(result.current.elapsedMs).toBe(4200)
    expect(result.current.level).toBe(0.5)
  })

  it("sets error when a command rejects", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_sources") return Promise.resolve([])
      if (cmd === "pause_recording") {
        return Promise.reject({
          message: "Cannot pause unless recording",
          recoverable: true,
        })
      }
      return Promise.resolve()
    })

    const { result } = renderHook(() => useRecordingState())
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("list_sources"))

    await act(async () => {
      await result.current.pauseRecording()
    })

    expect(result.current.error).toBe("Cannot pause unless recording")
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd apps/sound-app
pnpm vitest run src/hooks/useRecordingState.test.ts
```
Expected: FAIL — `./useRecordingState` module doesn't exist yet.

- [ ] **Step 3: Implement the hook**

Create `apps/sound-app/src/hooks/useRecordingState.ts`:

```tsx
import { useCallback, useEffect, useState } from "react"

import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"

export type AudioSource = { id: string; name: string }

export type RecordingState =
  | { state: "Idle" }
  | { state: "Preparing" }
  | { state: "Recording"; source_name: string; elapsed_ms: number }
  | { state: "Paused"; source_name: string; elapsed_ms: number }
  | { state: "Saving" }
  | {
      state: "Saved"
      file_path: string
      duration_ms: number
      size_bytes: number
    }
  | { state: "Error"; message: string; recoverable: boolean }

type CommandError = { message: string; recoverable: boolean }

type Tick = { elapsed_ms: number; level: number }

function errorMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as CommandError).message)
  }
  return String(err)
}

export function useRecordingState() {
  const [state, setState] = useState<RecordingState>({ state: "Idle" })
  const [elapsedMs, setElapsedMs] = useState(0)
  const [level, setLevel] = useState(0)
  const [sources, setSources] = useState<AudioSource[]>([])
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let unlistenState: (() => void) | undefined
    let unlistenTick: (() => void) | undefined

    listen<RecordingState>("recording-state-changed", (event) => {
      setState(event.payload)
    }).then((fn) => {
      unlistenState = fn
    })

    listen<Tick>("recording-tick", (event) => {
      setElapsedMs(event.payload.elapsed_ms)
      setLevel(event.payload.level)
    }).then((fn) => {
      unlistenTick = fn
    })

    invoke<AudioSource[]>("list_sources")
      .then(setSources)
      .catch((err) => setError(errorMessage(err)))

    return () => {
      unlistenState?.()
      unlistenTick?.()
    }
  }, [])

  const startRecording = useCallback(async (sourceId: string) => {
    setError(null)
    try {
      await invoke("start_recording", { sourceId })
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const pauseRecording = useCallback(async () => {
    setError(null)
    try {
      await invoke("pause_recording")
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const resumeRecording = useCallback(async () => {
    setError(null)
    try {
      await invoke("resume_recording")
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const stopRecording = useCallback(async () => {
    setError(null)
    try {
      await invoke("stop_recording")
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const cancelRecording = useCallback(async () => {
    setError(null)
    try {
      await invoke("cancel_recording")
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  return {
    state,
    elapsedMs,
    level,
    sources,
    error,
    startRecording,
    pauseRecording,
    resumeRecording,
    stopRecording,
    cancelRecording,
  }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd apps/sound-app
pnpm vitest run src/hooks/useRecordingState.test.ts
```
Expected: PASS — 4 tests.

- [ ] **Step 5: Verify typecheck and lint**

```bash
cd apps/sound-app
pnpm typecheck
pnpm lint
```
Expected: both clean, zero errors/warnings.

- [ ] **Step 6: Commit**

```bash
cd /home/tsemach/projects/sound-recorder
git add apps/sound-app/src/hooks
git commit -m "feat(sound-app): add useRecordingState hook"
```

---

## Task 6: `App.tsx` recording UI (TDD)

**Files:**
- Modify: `apps/sound-app/src/App.tsx` (full replacement — the "Project ready!"/ping content from PR 1/2 is deleted entirely)
- Modify: `apps/sound-app/src/App.test.tsx` (full replacement)

**Interfaces:**
- Consumes: `useRecordingState` and `RecordingState` from Task 5 (exact names/shapes).
- Produces: nothing further tasks depend on.

`apps/sound-app/src/App.tsx` and `apps/sound-app/src/App.test.tsx` currently (from PR 2) contain the `ping`-button demo — read both files first to confirm they match that description before replacing them.

- [ ] **Step 1: Write the failing tests**

Replace `apps/sound-app/src/App.test.tsx` entirely with:

```tsx
import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { App } from "./App"
import { useRecordingState } from "./hooks/useRecordingState"
import type { RecordingState } from "./hooks/useRecordingState"

vi.mock("./hooks/useRecordingState")

const mockUseRecordingState = vi.mocked(useRecordingState)

function baseHookReturn(
  overrides: Partial<ReturnType<typeof useRecordingState>> = {}
): ReturnType<typeof useRecordingState> {
  return {
    state: { state: "Idle" },
    elapsedMs: 0,
    level: 0,
    sources: [{ id: "fake-system-audio", name: "Fake System Audio" }],
    error: null,
    startRecording: vi.fn(),
    pauseRecording: vi.fn(),
    resumeRecording: vi.fn(),
    stopRecording: vi.fn(),
    cancelRecording: vi.fn(),
    ...overrides,
  }
}

describe("App", () => {
  beforeEach(() => {
    mockUseRecordingState.mockReset()
  })

  it("toggles dark mode when the d key is pressed", async () => {
    mockUseRecordingState.mockReturnValue(baseHookReturn())
    render(<App />)
    fireEvent.keyDown(window, { key: "d" })
    await waitFor(() => expect(document.documentElement).toHaveClass("dark"))
  })

  it("shows Record button and source select when idle", () => {
    mockUseRecordingState.mockReturnValue(baseHookReturn())
    render(<App />)
    expect(screen.getByRole("button", { name: "Record" })).toBeInTheDocument()
    expect(screen.getByRole("combobox")).toBeInTheDocument()
  })

  it("calls startRecording with the selected source when Record is clicked", () => {
    const startRecording = vi.fn()
    mockUseRecordingState.mockReturnValue(baseHookReturn({ startRecording }))
    render(<App />)
    fireEvent.click(screen.getByRole("button", { name: "Record" }))
    expect(startRecording).toHaveBeenCalledWith("fake-system-audio")
  })

  it("shows Pause, Stop, Cancel (not Record) while recording", () => {
    const recording: RecordingState = {
      state: "Recording",
      source_name: "Fake System Audio",
      elapsed_ms: 5000,
    }
    mockUseRecordingState.mockReturnValue(baseHookReturn({ state: recording }))
    render(<App />)
    expect(screen.getByRole("button", { name: "Pause" })).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "Record" })
    ).not.toBeInTheDocument()
  })

  it("shows Resume while paused", () => {
    const paused: RecordingState = {
      state: "Paused",
      source_name: "Fake System Audio",
      elapsed_ms: 5000,
    }
    mockUseRecordingState.mockReturnValue(baseHookReturn({ state: paused }))
    render(<App />)
    expect(screen.getByRole("button", { name: "Resume" })).toBeInTheDocument()
  })

  it("formats elapsed time as mm:ss", () => {
    const recording: RecordingState = {
      state: "Recording",
      source_name: "Fake",
      elapsed_ms: 65000,
    }
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({ state: recording, elapsedMs: 65000 })
    )
    render(<App />)
    expect(screen.getByText("01:05")).toBeInTheDocument()
  })

  it("renders the error banner when present", () => {
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({
        error: "Cannot start recording from the current state",
      })
    )
    render(<App />)
    expect(
      screen.getByText("Cannot start recording from the current state")
    ).toBeInTheDocument()
  })

  it("confirms before cancelling", () => {
    const cancelRecording = vi.fn()
    vi.spyOn(window, "confirm").mockReturnValue(true)
    const recording: RecordingState = {
      state: "Recording",
      source_name: "Fake",
      elapsed_ms: 1000,
    }
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({ state: recording, cancelRecording })
    )
    render(<App />)
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    expect(window.confirm).toHaveBeenCalled()
    expect(cancelRecording).toHaveBeenCalled()
  })
})
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd apps/sound-app
pnpm test
```
Expected: FAIL — `App.tsx` still has the old ping content, so none of the new queries (`"Record"` button, `combobox`, etc.) match anything.

- [ ] **Step 3: Implement the new `App.tsx`**

Replace `apps/sound-app/src/App.tsx` entirely with:

```tsx
import { useState } from "react"

import { Button } from "@workspace/ui/components/button"

import { ThemeProvider } from "./components/theme-provider"
import { useRecordingState } from "./hooks/useRecordingState"

function formatElapsed(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
}

export function App() {
  const {
    state,
    elapsedMs,
    level,
    sources,
    error,
    startRecording,
    pauseRecording,
    resumeRecording,
    stopRecording,
    cancelRecording,
  } = useRecordingState()

  const [sourceOverride, setSourceOverride] = useState<string | null>(null)
  const selectedSourceId = sourceOverride ?? sources[0]?.id ?? ""

  const canStart =
    state.state === "Idle" ||
    state.state === "Saved" ||
    (state.state === "Error" && state.recoverable)
  const isRecording = state.state === "Recording"
  const isPaused = state.state === "Paused"
  const isActive = isRecording || isPaused

  function handleCancel() {
    if (window.confirm("Discard this recording?")) {
      void cancelRecording()
    }
  }

  return (
    <ThemeProvider>
      <div className="flex min-h-svh flex-col gap-4 p-6">
        <h1 className="font-medium">Sound Recorder</h1>

        {error && (
          <div className="border-destructive text-destructive rounded border p-2 text-sm">
            {error}
          </div>
        )}

        {canStart && sources.length > 0 && (
          <select
            className="w-fit rounded border p-2 text-sm"
            value={selectedSourceId}
            onChange={(e) => setSourceOverride(e.target.value)}
          >
            {sources.map((source) => (
              <option key={source.id} value={source.id}>
                {source.name}
              </option>
            ))}
          </select>
        )}

        <div className="font-mono text-2xl">
          {formatElapsed(isActive ? elapsedMs : 0)}
        </div>

        {isActive && (
          <div className="bg-muted h-2 w-full max-w-xs rounded">
            <div
              className="bg-primary h-2 rounded transition-all"
              style={{ width: `${Math.min(level, 1) * 100}%` }}
            />
          </div>
        )}

        <div className="flex gap-2">
          {canStart && (
            <Button onClick={() => void startRecording(selectedSourceId)}>
              Record
            </Button>
          )}
          {isRecording && (
            <Button onClick={() => void pauseRecording()}>Pause</Button>
          )}
          {isPaused && (
            <Button onClick={() => void resumeRecording()}>Resume</Button>
          )}
          {isActive && <Button onClick={() => void stopRecording()}>Stop</Button>}
          {isActive && <Button onClick={handleCancel}>Cancel</Button>}
        </div>
      </div>
    </ThemeProvider>
  )
}
```

Note: `selectedSourceId` is computed at render time from `sourceOverride ?? sources[0]?.id ?? ""` rather than synced into state via a `useEffect` — deriving default/initial values in an effect is exactly the anti-pattern `eslint-plugin-react-hooks`'s `set-state-in-effect` rule flags (confirmed by triggering it during planning). `sourceOverride` only holds an explicit user selection (`null` until they interact with the `<select>`).

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd apps/sound-app
pnpm test
```
Expected: PASS — 8 tests in `App.test.tsx`, plus the 4 in `useRecordingState.test.ts` (12 total).

- [ ] **Step 5: Verify typecheck and lint**

```bash
cd apps/sound-app
pnpm typecheck
pnpm lint
```
Expected: both clean, zero errors/warnings.

- [ ] **Step 6: Commit**

```bash
cd /home/tsemach/projects/sound-recorder
git add apps/sound-app/src/App.tsx apps/sound-app/src/App.test.tsx
git commit -m "feat(sound-app): replace ping demo with real recording UI"
```

---

## Task 7: End-to-end verification and docs

**Files:**
- Modify: `CLAUDE.md`

**Interfaces:** none — this is the final integration/verification task.

- [ ] **Step 1: Manual real-window verification**

```bash
cd apps/sound-app
pnpm tauri dev
```

This opens an actual native window on this machine's Wayland display. In that window:
- Confirm a source `<select>` appears with "Fake System Audio" / "Fake Microphone" options.
- Confirm a "Record" button is visible.
- Click Record. Confirm the button row changes to Pause/Stop/Cancel, and the elapsed timer starts counting up.
- Watch the level meter bar animate (it should visibly fluctuate, not stay flat, since `FakeCapture` generates a real sine wave).
- Click Pause. Confirm the button changes to Resume, and the timer stops advancing.
- Click Resume. Confirm it goes back to Recording and the timer continues from where it paused.
- Click Stop. Confirm the state returns to something startable again (Record button reappears).
- Start a new recording, then click Cancel. Confirm a native confirm dialog appears; confirming it should return to the idle Record-button state.
- Open the devtools console if possible, or just note: one benign "IPC custom protocol failed... postMessage" warning on startup is expected (see Global Constraints) — anything else unexpected should be investigated before proceeding.

If any of this doesn't work as described, STOP and report — this is the one check that proves the real IPC bridge and UI work together, not the isolated unit/component tests.

- [ ] **Step 2: Update `CLAUDE.md`**

Find the "Project" section's `sound-app` bullet (currently mentions the Tauri shell existing per PR 2, with recording/capture logic still pending) and the "Repo structure" section's `sound-app` bullet. Read both current bullets first, then update them to reflect that:
- The recording state machine, commands, and UI now exist (backed by fake/mocked capture, not real audio yet).
- `pnpm --filter sound-app test:rust` runs the Rust test suite (in addition to the existing `cargo test` note).

Keep the edits scoped to just updating what's now inaccurate — don't rewrite either section wholesale.

- [ ] **Step 3: Full verification suite**

```bash
cd /home/tsemach/projects/sound-recorder
pnpm build
pnpm lint
pnpm typecheck
pnpm test
pnpm test:rust
cd apps/sound-app/src-tauri
cargo fmt --check
cargo clippy --all-targets
git status
```
All `pnpm`/`cargo` commands must exit 0. `git status` should show a clean tree after the final commit.

- [ ] **Step 4: Commit**

```bash
cd /home/tsemach/projects/sound-recorder
git add CLAUDE.md
git commit -m "docs: note recording state machine is now present in sound-app"
```

---

## Roadmap: remaining PRs (from the approved migration spec, planned in detail when reached)

4. **Real Linux capture** — `libpulse-binding` integration, `LinuxPulseCapture` implementing the same `AudioCapture` trait this PR defines, real `list_sources()`. `FakeCapture` gets swapped out; the state machine, commands, and tick/level computation are untouched.
5. **WAV writer + atomic finalize** — incremental PCM writes to temp file, header patch + atomic rename on stop, orphaned-temp-file recovery. `stop_recording`'s fake `file_path`/`size_bytes` become real.
6. **Storage checks + error surfacing** — disk-space checks, richer `Error` state UI (this PR's error banner is a plain message; PR 6 can build on it).
7. **Recordings list screen** — `list_recordings`/`delete_recording`/`rename_recording` commands (deferred from this PR) plus the UI to use them.
8. **Settings screen** — save-location picker, filename template, source/quality selection persisted.
