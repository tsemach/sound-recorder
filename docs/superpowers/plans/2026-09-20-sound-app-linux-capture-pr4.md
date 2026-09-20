# sound-app: Real Linux Capture (PR 4) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `FakeCapture` with a real `LinuxPulseCapture` that captures actual system audio via PulseAudio/PipeWire monitor sources, while widening the `AudioCapture` trait to carry audio format info and asynchronous capture errors — the two gaps PR 3's final review found.

**Architecture:** Widen `AudioCapture`'s `FrameCallback` to `Result<Vec<i16>, CaptureError>` and add a `format() -> AudioFormat` method; propagate that through `FakeCapture`, `tick.rs`'s duration math, `SharedState`, and `commands.rs`'s frame callback (still backed by `FakeCapture`, zero behavior change). Then add `LinuxPulseCapture` as a new, additive implementation using `libpulse-binding`'s standard `Mainloop` for source enumeration and `libpulse-simple-binding`'s blocking `Simple` API for streaming capture on a background thread. Finally swap `FakeCapture` for `LinuxPulseCapture` in `lib.rs`'s production wiring.

**Tech Stack:** Rust, Tauri v2, `libpulse-binding = "2"` (resolves to 2.30.1), `libpulse-simple-binding = "2"` (resolves to 2.29.0).

**Spec:** `docs/superpowers/specs/2026-09-20-sound-app-linux-capture-design.md`

## Global Constraints

- No new system packages are required — `libpulse-sys`/`libpulse-simple-sys` link against this machine's existing PulseAudio client library (verified: `cargo build` with the new deps succeeds with zero `sudo dnf install`).
- All commands stay plain sync `fn` — no `async` needed anywhere in this PR. PR 3's check-then-act race deferral (documented in `commands.rs`'s `emit_state` comment) remains valid and is **not** reopened by this PR.
- `AudioSource` (`{ id, name }`) is unchanged — format info flows only through `AudioCapture::format()`, never through the source list.
- Only real monitor sources (`monitor_of_sink.is_some()`) are exposed — real microphone inputs are filtered out, matching the PRD's no-microphone-capture non-goal.
- Real file writing is out of scope (PR 5) — `stop_recording` still emits the fake `file_path`/`size_bytes` placeholders in `Saved`, now with a real `duration_ms`.
- Every task's commit must leave `cargo test`, `cargo clippy --all-targets`, and `cargo fmt --check` all clean (zero warnings) — this repo's established gate since PR 3.

---

## Context for the implementer

Current repo state (all on `master`, this branch `feat/sound-app-linux-capture` forks from it): `apps/sound-app/src-tauri` is a working Tauri v2 app with a full recording state machine (PR 3) backed entirely by `FakeCapture` (a synthetic sine-wave generator). The five files this PR touches or adds:

- `src/capture/mod.rs` — the `AudioCapture` trait, `AudioSource`, `CaptureError`, `FrameCallback` type alias.
- `src/capture/fake.rs` — `FakeCapture`, the only current trait implementor.
- `src/capture/linux_pulse.rs` — **new file**, this PR's main deliverable.
- `src/tick.rs` — `buffer_duration_ms`/`compute_level`, pure functions used by the frame callback.
- `src/state.rs` — `RecordingState`, `SharedState` (Tauri-managed state).
- `src/commands.rs` — the 6 `#[tauri::command]` functions and `make_frame_callback`.
- `src/lib.rs` — wires `SharedState::new(Box::new(FakeCapture::new()))` and registers commands.

All code in this plan was written and validated by actually compiling and running it against this exact crate (including a real end-to-end capture test against this machine's real PipeWire/Pulse daemon) before being written into this document — it is not guessed syntax.

---

### Task 1: Widen the `AudioCapture` trait and adapt the existing fake-backed pipeline

This task changes the trait shape (`FrameCallback` now delivers a `Result`, plus a new `format()` method) and propagates that change through every file that currently depends on the old shape, so the crate keeps compiling and all existing behavior is unchanged (still backed by `FakeCapture`). These five files are edited together because the type change is not separable for compilation: changing `FrameCallback`'s alias immediately breaks every existing closure built against the old signature.

**Files:**
- Modify: `apps/sound-app/src-tauri/src/capture/mod.rs`
- Modify: `apps/sound-app/src-tauri/src/capture/fake.rs`
- Modify: `apps/sound-app/src-tauri/src/tick.rs`
- Modify: `apps/sound-app/src-tauri/src/state.rs`
- Modify: `apps/sound-app/src-tauri/src/commands.rs`

**Interfaces:**
- Produces (used by Task 2 and Task 3):
  - `pub struct AudioFormat { pub sample_rate: u32, pub channels: u8 }` — `Debug, Clone, Copy`, in `capture/mod.rs`.
  - `pub type FrameCallback = Box<dyn Fn(Result<Vec<i16>, CaptureError>) + Send + 'static>`.
  - `fn format(&self) -> AudioFormat` added to the `AudioCapture` trait.
  - `pub fn buffer_duration_ms(sample_count: usize, sample_rate: u32, channels: u8) -> u64` in `tick.rs` (signature change from the current single-argument form).
  - `SharedState.state: Arc<Mutex<RecordingState>>` (widened from `Mutex<RecordingState>`).
  - `SharedState.format: Arc<Mutex<AudioFormat>>` — new field, the frame callback's format cache.

- [ ] **Step 1: Widen `capture/mod.rs`**

Replace the full file:

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

#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
  pub sample_rate: u32,
  pub channels: u8,
}

pub type FrameCallback = Box<dyn Fn(Result<Vec<i16>, CaptureError>) + Send + 'static>;

pub trait AudioCapture: Send {
  fn list_sources(&self) -> Result<Vec<AudioSource>, CaptureError>;
  fn start(&mut self, source_id: &str, on_frame: FrameCallback) -> Result<(), CaptureError>;
  fn format(&self) -> AudioFormat;
  fn pause(&mut self);
  fn resume(&mut self);
  fn stop(&mut self) -> Result<(), CaptureError>;
}
```

(Task 2 later adds `pub mod linux_pulse;` to this file — do not add it now, `linux_pulse.rs` doesn't exist yet and this task must compile on its own.)

- [ ] **Step 2: Run a build to confirm the expected breakage**

Run: `cd apps/sound-app/src-tauri && cargo check --all-targets`
Expected: FAIL — `capture/fake.rs` no longer implements the trait (missing `format()`, wrong `FrameCallback` payload type) and `commands.rs`'s closures no longer match. This confirms the trait change took effect; the next steps fix each broken call site.

- [ ] **Step 3: Adapt `capture/fake.rs`**

Add `AudioFormat` to the import, add the `format()` method, and wrap every `on_frame` call in `Ok(...)`:

```rust
use super::{AudioCapture, AudioFormat, AudioSource, CaptureError, FrameCallback};
```

In the `start` method, change:

```rust
        on_frame(buffer);
```

to:

```rust
        on_frame(Ok(buffer));
```

Add this method to the `impl AudioCapture for FakeCapture` block (any position — placing it right after `start` matches the trait's declared order):

```rust
  fn format(&self) -> AudioFormat {
    AudioFormat {
      sample_rate: crate::tick::SAMPLE_RATE_HZ as u32,
      channels: 1,
    }
  }
```

Then update both test closures (in `#[cfg(test)] mod tests`) that currently read:

```rust
        Box::new(move |buffer| {
          received_cb.lock().unwrap().push(buffer);
        }),
```

to:

```rust
        Box::new(move |result| {
          received_cb.lock().unwrap().push(result.unwrap());
        }),
```

There are two occurrences (`produces_frames_while_running` and `pause_stops_producing_frames`) — update both.

- [ ] **Step 4: Verify `fake.rs`'s own tests pass**

Run: `cargo test --lib capture::fake`
Expected: PASS (3 tests: `produces_frames_while_running`, `pause_stops_producing_frames`, `list_sources_returns_fake_entries`).

- [ ] **Step 5: Make `tick.rs`'s `buffer_duration_ms` format-aware**

Replace the full file:

```rust
pub const SAMPLE_RATE_HZ: u64 = 48_000;

/// Duration in milliseconds represented by `sample_count` interleaved samples
/// at the given sample rate and channel count. `sample_count` counts total
/// i16 values in the buffer (all channels combined), matching what
/// `AudioCapture` frame callbacks receive.
pub fn buffer_duration_ms(sample_count: usize, sample_rate: u32, channels: u8) -> u64 {
  let frames = sample_count as u64 / channels.max(1) as u64;
  (frames * 1000) / sample_rate.max(1) as u64
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
  fn buffer_duration_matches_sample_rate_mono() {
    // 48 mono samples at 48kHz = 1ms
    assert_eq!(buffer_duration_ms(48, 48_000, 1), 1);
    // 960 mono samples at 48kHz = 20ms
    assert_eq!(buffer_duration_ms(960, 48_000, 1), 20);
  }

  #[test]
  fn buffer_duration_accounts_for_channel_count() {
    // 1920 interleaved samples = 960 stereo frames at 48kHz = 20ms
    assert_eq!(buffer_duration_ms(1920, 48_000, 2), 20);
  }

  #[test]
  fn buffer_duration_accounts_for_sample_rate() {
    // 441 mono samples at 44.1kHz = 10ms
    assert_eq!(buffer_duration_ms(441, 44_100, 1), 10);
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

- [ ] **Step 6: Verify `tick.rs`'s own tests pass**

Run: `cargo test --lib tick::`
Expected: PASS (6 tests).

- [ ] **Step 7: Widen `state.rs`'s `SharedState`**

Change the import line:

```rust
use crate::capture::AudioCapture;
```

to:

```rust
use crate::capture::{AudioCapture, AudioFormat};
```

Remove the now-inapplicable comment and `#[allow(dead_code)]` above the `Error` variant (this PR makes `commands.rs`'s frame callback a real producer of it), so the enum's tail becomes:

```rust
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
```

Replace the `SharedState` struct and its `impl` block:

```rust
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
    }
  }
}
```

Nothing else in `state.rs` changes — all of `RecordingState`'s methods and the existing `#[cfg(test)] mod tests` block (which tests `RecordingState` directly, not `SharedState`) are unaffected.

- [ ] **Step 8: Verify `state.rs`'s own tests pass**

Run: `cargo test --lib state::`
Expected: PASS (6 tests, unchanged).

- [ ] **Step 9: Adapt `commands.rs`'s frame callback and `try_start`**

Change the import line:

```rust
use crate::capture::{AudioSource, FrameCallback};
```

to:

```rust
use crate::capture::{AudioFormat, AudioSource, FrameCallback};
```

In `try_start`, replace this block:

```rust
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
    .start(source_id, on_frame)
    .map_err(|e| CommandError::new(e.message))?;

  Ok(source_name)
}
```

with:

```rust
  let on_frame = make_frame_callback(
    app.clone(),
    Arc::clone(&state.elapsed_ms),
    Arc::clone(&state.level),
    Arc::clone(&state.last_tick_emit),
    Arc::clone(&state.format),
    Arc::clone(&state.state),
  );

  let mut capture = state.capture.lock().unwrap();
  capture
    .start(source_id, on_frame)
    .map_err(|e| CommandError::new(e.message))?;
  *state.format.lock().unwrap() = capture.format();

  Ok(source_name)
}
```

(The lock guard is held across `.start()` and the immediate `.format()` read since both need `capture` — this is not a behavior change, `.start()` was already called while holding the lock before.)

Replace the `make_frame_callback` function:

```rust
fn make_frame_callback(
  app: AppHandle,
  elapsed_ms: Arc<Mutex<u64>>,
  level: Arc<Mutex<f32>>,
  last_tick_emit: Arc<Mutex<Instant>>,
  format: Arc<Mutex<AudioFormat>>,
  state: Arc<Mutex<RecordingState>>,
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
    },
  )
}
```

Also update the doc comment directly above `make_frame_callback` (currently says "Each fake (or, later, real) PCM buffer") — since this PR makes it real, change it to:

```rust
/// Builds the callback passed to `AudioCapture::start`. Each PCM buffer updates the
/// shared elapsed/level counters and emits a throttled `recording-tick` — or, if the
/// capture reports a failure, transitions straight to `RecordingState::Error`.
/// Takes `Arc` clones rather than a `SharedState`/`State` reference because this closure
/// must be `'static` (it runs on the capture's background thread), and `tauri::State` is
/// only valid for the duration of the command invocation that produced it.
```

No other command (`pause_recording`, `resume_recording`, `stop_recording`, `cancel_recording`, `list_sources`) needs any change — they don't touch `FrameCallback` or `AudioFormat`, and `state.state.lock()` reads exactly the same way whether `state` is a `Mutex` or an `Arc<Mutex<..>>` (both `Deref` to the guard).

- [ ] **Step 10: Full verification for this task**

Run, from `apps/sound-app/src-tauri`:
```bash
cargo test
cargo clippy --all-targets
cargo fmt --check
```
Expected: all three clean — 13 tests passing (the same count as before this task; no new tests were added, only signatures changed), zero clippy warnings, no formatting diff.

- [ ] **Step 11: Commit**

```bash
git add src/capture/mod.rs src/capture/fake.rs src/tick.rs src/state.rs src/commands.rs
git commit -m "feat(sound-app): widen AudioCapture for async errors and audio format"
```

---

### Task 2: Implement `LinuxPulseCapture`

Adds the real capture engine as a new, additive file. Nothing from Task 1's pipeline is touched here — `lib.rs` still wires up `FakeCapture` until Task 3.

**Files:**
- Modify: `apps/sound-app/src-tauri/Cargo.toml` (add two dependencies)
- Modify: `apps/sound-app/src-tauri/Cargo.lock` (regenerated by `cargo build`)
- Modify: `apps/sound-app/src-tauri/src/capture/mod.rs` (add `pub mod linux_pulse;`)
- Create: `apps/sound-app/src-tauri/src/capture/linux_pulse.rs`

**Interfaces:**
- Consumes: `AudioCapture`, `AudioFormat`, `AudioSource`, `CaptureError`, `FrameCallback` from Task 1's `capture/mod.rs`.
- Produces (used by Task 3): `pub struct LinuxPulseCapture` implementing `AudioCapture`, with `pub fn new() -> Self`.

- [ ] **Step 1: Add the libpulse dependencies**

In `apps/sound-app/src-tauri/Cargo.toml`, add to the `[dependencies]` section (after `tauri-plugin-log = "2"`):

```toml
libpulse-binding = "2"
libpulse-simple-binding = "2"
```

Run: `cd apps/sound-app/src-tauri && cargo build`
Expected: succeeds, resolving `libpulse-binding` to `2.30.1` and `libpulse-simple-binding` to `2.29.0` (verified versions; no `sudo dnf install` needed — this machine's existing PulseAudio client library is sufficient). This updates `Cargo.lock`.

- [ ] **Step 2: Register the new module**

In `apps/sound-app/src-tauri/src/capture/mod.rs`, change the first line from:

```rust
pub mod fake;
```

to:

```rust
pub mod fake;
pub mod linux_pulse;
```

- [ ] **Step 3: Create `capture/linux_pulse.rs`**

```rust
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

/// Converts a raw little/native-endian PCM byte buffer (as read from
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

  Ok(
    Rc::try_unwrap(sources)
      .map(|cell| cell.into_inner())
      .unwrap_or_default(),
  )
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
      .find(|s| s.name == source_id)
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

    capture
      .start(
        &sources[0].id,
        Box::new(move |result| {
          received_cb.lock().unwrap().push(result);
        }),
      )
      .expect("start should succeed against a real, reachable monitor source");

    thread::sleep(Duration::from_millis(200));
    capture.stop().unwrap();

    let frames = received.lock().unwrap();
    assert!(
      !frames.is_empty(),
      "expected at least one real frame (or a reported error) within 200ms"
    );
    assert!(
      matches!(frames[0], Ok(ref samples) if !samples.is_empty()),
      "expected the first real frame to be a non-empty Ok(...) sample buffer"
    );
  }
}
```

Notes on the two real-daemon tests (`list_sources_against_real_daemon`, `real_capture_produces_at_least_one_frame_or_skips_gracefully`): both degrade gracefully (print a warning and return early, do not fail) if no daemon is reachable or no monitor source exists — per the spec's testing strategy, since this repo has no CI yet and these tests must not become a landmine on a different machine.

- [ ] **Step 4: Run the full test suite**

Run: `cargo test`
Expected: PASS. On this machine, all 21 tests pass, including both real-daemon tests actually exercising this machine's real PipeWire/Pulse-compatible daemon (verified: `real_capture_produces_at_least_one_frame_or_skips_gracefully` captured a real non-empty frame from the real monitor source within the 200ms window). On a machine with no daemon, the same two tests print a warning and pass trivially.

- [ ] **Step 5: Lint and format**

Run:
```bash
cargo clippy --all-targets
cargo fmt --check
```
Expected: both clean.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/capture/mod.rs src/capture/linux_pulse.rs
git commit -m "feat(sound-app): add LinuxPulseCapture using libpulse-binding"
```

---

### Task 3: Wire `LinuxPulseCapture` into production and verify end-to-end

**Files:**
- Modify: `apps/sound-app/src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `LinuxPulseCapture::new()` from Task 2.

- [ ] **Step 1: Swap the capture implementation**

In `apps/sound-app/src-tauri/src/lib.rs`, change:

```rust
use capture::fake::FakeCapture;
use state::SharedState;
```

to:

```rust
use capture::linux_pulse::LinuxPulseCapture;
use state::SharedState;
```

and change:

```rust
    .manage(SharedState::new(Box::new(FakeCapture::new())))
```

to:

```rust
    .manage(SharedState::new(Box::new(LinuxPulseCapture::new())))
```

- [ ] **Step 2: Full automated verification**

Run: `cargo test`
Expected: PASS, same test count as Task 2's Step 4.

Run: `cargo clippy --all-targets`
Expected: 5 `dead_code` warnings on `capture/fake.rs` (the struct, its `new()`, and two constants) — verified: swapping `FakeCapture` out of `lib.rs`'s production wiring means nothing outside `fake.rs`'s own tests constructs it anymore, and `cargo clippy`'s reachability analysis flags exactly that. Fix this now by adding a module-level attribute at the very top of `capture/fake.rs` (before the existing `use` statements):

```rust
// Kept for its own test coverage of the AudioCapture contract (a fast,
// deterministic reference implementation) — production wiring uses
// LinuxPulseCapture as of PR 4, so nothing outside this module's tests
// constructs FakeCapture anymore.
#![allow(dead_code)]
```

Run `cargo clippy --all-targets` again: expected zero warnings.

Run: `cargo fmt --check`
Expected: no diff.

- [ ] **Step 3: Manual verification**

Run: `pnpm --filter sound-app tauri dev`

In the launched app window:
1. Confirm the source dropdown shows this machine's real monitor source (e.g. "Monitor of Built-in Audio Analog Stereo"), not `FakeCapture`'s "Fake System Audio"/"Fake Microphone" entries.
2. Click Record, then play some audio on the machine (e.g. a YouTube video or music) — watch the level meter move with the audio instead of staying flat.
3. Play silence (pause the audio) — confirm the level meter drops back toward zero.
4. Click Pause, then Resume — confirm the elapsed timer pauses and resumes correctly.
5. Click Stop — confirm it transitions to a "Saved" state with a real, non-zero `duration_ms` reflected in the final elapsed time shown before stopping.
6. Click Record again, then Cancel while recording — confirm it discards and returns to idle without error.

This is a UI/behavior change (from fake to real audio) — it must be checked in the actual running app, not just by passing automated tests.

- [ ] **Step 4: Commit**

```bash
git add src/lib.rs src/capture/fake.rs
git commit -m "feat(sound-app): switch production capture to LinuxPulseCapture"
```

---

## Verification (whole plan)

```bash
cd apps/sound-app/src-tauri
cargo test               # all tests pass, including real-daemon capture tests
cargo clippy --all-targets  # zero warnings
cargo fmt --check         # no diff
cd ../../..
pnpm --filter sound-app tauri dev   # manual click-through per Task 3 Step 3
```

## Explicitly out of scope for this PR

- Real file writing (PR 5's WAV writer) — `stop_recording` still emits fake `file_path`/`size_bytes`.
- Windows/macOS capture implementations.
- Storage/disk-space checks (PR 6).
- The check-then-act race across commands — confirmed still deferred, correctly, since nothing in this PR requires async commands.
- Any change to `RecordingState`'s shape, transition predicates, or the command/event surface — this PR only swaps the capture implementation and widens the trait it implements.
- `list_recordings`/`delete_recording`/`rename_recording` and the recordings list screen (PR 7).
