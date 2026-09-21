# sound-app: WAV Writer + Atomic Finalize (PR 5) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the fake `Saved{file_path: "fake-recording.wav", size_bytes: 0}` placeholder with a real WAV file, written incrementally to a temp file on a dedicated writer thread and atomically finalized on stop.

**Architecture:** A new `writer.rs` module spawns a writer thread per recording that owns a `hound::WavWriter`; the frame callback forwards PCM frames to it over an `mpsc` channel (keeping disk I/O off the capture thread). The writer calls `hound`'s `flush()` after every frame, so the temp file is always readable up to the last flush even if the process is killed outright — a new `recovery.rs` module scans for and promotes/deletes any such orphaned temp files at startup. A new `try_transition` helper in `state.rs` closes a stale-overwrite race in `stop_recording`/`cancel_recording` that a prior PR's review flagged as a prerequisite for this one.

**Tech Stack:** Rust, `hound = "3.5.1"` (WAV I/O), `chrono = "0.4.45"` (timestamped filenames), Tauri v2's `app.path().audio_dir()`.

**Spec:** `docs/superpowers/specs/2026-09-21-sound-app-wav-writer-design.md`

## Global Constraints

- Save directory: `<OS audio dir>/Sound Recorder/`, created on first use.
- Filenames: `recording-YYYY-MM-DD_HH-MM-SS.wav` (final), `<same>.tmp` (in-progress).
- No new Tauri commands become `async` — the writer thread is a plain `std::thread`, not a Tokio task. This must not reopen the async-command race PR 3/PR 4 deliberately kept closed.
- `AudioCapture::format()`/`SharedState.format` is the only source of truth for the WAV header's sample rate/channels — never `tick::SAMPLE_RATE_HZ`.
- Guarded-transition scope: only `stop_recording`/`cancel_recording` get the reorder + `try_transition` treatment. `pause_recording`/`resume_recording`/`start_recording` are unchanged.
- Every task's commit must leave `cargo test`, `cargo clippy --all-targets`, and `cargo fmt --check` all clean (zero warnings) — this repo's established gate since PR 3.

---

## Context for the implementer

Current repo state (branch `feat/sound-app-wav-writer`, forked from `master` with PR 1-4 merged): `apps/sound-app/src-tauri` has a working Tauri v2 app with real Linux audio capture (PR 4). `commands.rs`'s `stop_recording` currently emits a hardcoded fake file — this PR makes it real. Files this PR touches or adds:

- `src/writer.rs` — **new**. Writer thread, message protocol, save-path helpers.
- `src/recovery.rs` — **new**. Orphaned-temp-file scan, run once at startup.
- `src/state.rs` — add `try_transition`, add `SharedState.writer` field.
- `src/commands.rs` — `try_start` spawns the writer; frame callback forwards frames to it; `stop_recording`/`cancel_recording` reordered and guarded.
- `src/lib.rs` — registers the two new modules; runs orphan recovery in `.setup()`.
- `Cargo.toml` — adds `hound` and `chrono`.

All code below was written by actually compiling and running it against this exact crate — including a full write→flush→simulated-crash→reopen round trip with `hound::WavReader`, and a full write→finalize→reopen round trip confirming exact sample/duration/size correctness — before being put in this document. It is not guessed syntax.

**A note on durability, discovered during validation (not in the original design doc, but strictly more robust than what it described):** `hound::WavWriter` wraps its file in a `BufWriter`. A killed process can lose whatever sits in that buffer unflushed — but `hound::WavWriter::flush()` is a purpose-built "checkpoint" method: it patches the header to the correct size *and* flushes to the OS, in one call. Calling it after every incoming frame means the temp file is valid and fully readable up to the last frame, at all times, with no separate manual byte-patching needed in the common case. The manual header patch (recomputing size fields from the file's raw byte count) is now only a fallback, for the narrow case of a crash before the very first flush ever completed.

---

### Task 1: Guarded transitions + writer subsystem

**Files:**
- Modify: `apps/sound-app/src-tauri/Cargo.toml` (add `hound`, `chrono`)
- Modify: `apps/sound-app/src-tauri/src/state.rs` (add `try_transition`)
- Create: `apps/sound-app/src-tauri/src/writer.rs`

**Interfaces:**
- Produces (used by Task 3):
  - `pub fn try_transition(state: &Arc<Mutex<RecordingState>>, allowed: impl Fn(&RecordingState) -> bool, next: RecordingState) -> bool` in `state.rs`.
  - `pub enum WriterMessage { Frame(Vec<i16>), Finalize, Discard }`
  - `pub struct WriterResult { pub file_path: String, pub duration_ms: u64, pub size_bytes: u64 }`
  - `pub struct WriterError { pub message: String }`
  - `pub struct WriterHandle { pub sender: std::sync::mpsc::Sender<WriterMessage>, pub join_handle: std::thread::JoinHandle<Option<WriterResult>> }`
  - `pub fn create_channel() -> (std::sync::mpsc::Sender<WriterMessage>, std::sync::mpsc::Receiver<WriterMessage>)`
  - `pub fn spawn_writer(receiver: std::sync::mpsc::Receiver<WriterMessage>, temp_path: PathBuf, final_path: PathBuf, format: AudioFormat, app: AppHandle, state: Arc<Mutex<RecordingState>>) -> Result<std::thread::JoinHandle<Option<WriterResult>>, WriterError>`
  - `pub fn recording_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String>`
  - `pub fn timestamped_wav_paths(dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf)` — returns `(temp_path, final_path)`.

- [ ] **Step 1: Add the dependencies**

Run: `cd apps/sound-app/src-tauri && cargo add hound chrono`
Expected: `Cargo.toml` gains `hound = "3.5.1"` and `chrono = "0.4.45"` (or the current latest-compatible resolution — don't hand-edit to force these exact strings, just run the command and let cargo resolve). `Cargo.lock` is regenerated.

- [ ] **Step 2: Add `try_transition` to `state.rs`, with tests**

In `apps/sound-app/src-tauri/src/state.rs`, add this function after the `SharedState` `impl` block (before the existing `#[cfg(test)] mod tests`):

```rust
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
```

Add these tests inside the existing `#[cfg(test)] mod tests` block (after the existing tests, before the closing `}`):

```rust
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
```

Run: `cargo test --lib state::`
Expected: PASS (8 tests: the existing 6 plus these 2 new ones).

- [ ] **Step 3: Create `writer.rs`**

```rust
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use tauri::{AppHandle, Manager};

use crate::capture::AudioFormat;
use crate::state::RecordingState;

pub enum WriterMessage {
  Frame(Vec<i16>),
  Finalize,
  Discard,
}

#[derive(Debug)]
pub struct WriterResult {
  pub file_path: String,
  pub duration_ms: u64,
  pub size_bytes: u64,
}

pub struct WriterError {
  pub message: String,
}

pub struct WriterHandle {
  pub sender: Sender<WriterMessage>,
  pub join_handle: thread::JoinHandle<Option<WriterResult>>,
}

/// Resolves (and creates, if missing) the default save directory:
/// `<OS audio dir>/Sound Recorder/`. No settings screen exists yet (PR 8) to
/// make this configurable.
pub fn recording_dir(app: &AppHandle) -> Result<PathBuf, String> {
  let audio_dir = app
    .path()
    .audio_dir()
    .map_err(|e| format!("Could not resolve audio directory: {e}"))?;
  let dir = audio_dir.join("Sound Recorder");
  std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create save directory: {e}"))?;
  Ok(dir)
}

/// Builds `(temp_path, final_path)` for a new recording started now.
/// `temp_path` is the final name with an extra `.tmp` suffix.
pub fn timestamped_wav_paths(dir: &Path) -> (PathBuf, PathBuf) {
  let now = std::time::SystemTime::now();
  let datetime: chrono::DateTime<chrono::Local> = now.into();
  let name = format!("recording-{}.wav", datetime.format("%Y-%m-%d_%H-%M-%S"));
  let final_path = dir.join(&name);
  let temp_path = dir.join(format!("{name}.tmp"));
  (temp_path, final_path)
}

/// Creates the channel used to hand frames from the capture thread's frame
/// callback to the writer thread. Split out from `spawn_writer` because the
/// `Sender` is needed by `make_frame_callback` before the real `AudioFormat`
/// (needed to create the `hound::WavWriter`) is known.
pub fn create_channel() -> (Sender<WriterMessage>, Receiver<WriterMessage>) {
  mpsc::channel()
}

/// Creates the temp WAV file and spawns the thread that owns it. Synchronous
/// (the `hound::WavWriter::create` call, and thus any permission/path
/// error, happens here before any thread is spawned) — matches the pattern
/// `LinuxPulseCapture::start` already uses for its own synchronous setup.
pub fn spawn_writer(
  receiver: Receiver<WriterMessage>,
  temp_path: PathBuf,
  final_path: PathBuf,
  format: AudioFormat,
  app: AppHandle,
  state: Arc<Mutex<RecordingState>>,
) -> Result<thread::JoinHandle<Option<WriterResult>>, WriterError> {
  let spec = hound::WavSpec {
    channels: format.channels as u16,
    sample_rate: format.sample_rate,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
  };
  let writer = hound::WavWriter::create(&temp_path, spec).map_err(|e| WriterError {
    message: format!("Could not create recording file: {e}"),
  })?;

  Ok(thread::spawn(move || {
    run_writer_loop(writer, receiver, format, temp_path, final_path, app, state)
  }))
}

fn run_writer_loop(
  mut writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
  receiver: Receiver<WriterMessage>,
  format: AudioFormat,
  temp_path: PathBuf,
  final_path: PathBuf,
  app: AppHandle,
  state: Arc<Mutex<RecordingState>>,
) -> Option<WriterResult> {
  let mut failed = false;

  loop {
    match receiver.recv() {
      Ok(WriterMessage::Frame(samples)) => {
        if failed {
          continue;
        }
        let mut write_err: Option<hound::Error> = None;
        for sample in samples {
          if let Err(e) = writer.write_sample(sample) {
            write_err = Some(e);
            break;
          }
        }
        if write_err.is_none() {
          write_err = writer.flush().err();
        }
        if let Some(e) = write_err {
          failed = true;
          let message = format!("Could not write audio to disk: {e}");
          *state.lock().unwrap() = RecordingState::Error {
            message: message.clone(),
            recoverable: true,
          };
          let _ = tauri::Emitter::emit(
            &app,
            "recording-state-changed",
            RecordingState::Error {
              message,
              recoverable: true,
            },
          );
        }
      }
      Ok(WriterMessage::Finalize) => {
        if failed {
          let _ = std::fs::remove_file(&temp_path);
          return None;
        }
        let duration_samples = writer.duration();
        let duration_ms = (duration_samples as u64 * 1000) / format.sample_rate.max(1) as u64;
        if writer.finalize().is_err() {
          return None;
        }
        let size_bytes = std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0);
        if std::fs::rename(&temp_path, &final_path).is_err() {
          return None;
        }
        return Some(WriterResult {
          file_path: final_path.to_string_lossy().to_string(),
          duration_ms,
          size_bytes,
        });
      }
      Ok(WriterMessage::Discard) => {
        drop(writer);
        let _ = std::fs::remove_file(&temp_path);
        return None;
      }
      Err(_) => {
        // Channel closed with no terminal message: the frame callback's
        // Sender was dropped after the capture thread exited due to an
        // error, with nobody sending Finalize/Discard. Drop the writer
        // (its periodic flush() calls mean the temp file is already valid
        // up to the last frame) and leave it for recovery::
        // recover_orphaned_recordings to promote or delete at next startup.
        return None;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn timestamped_names_have_the_expected_shape() {
    let dir = PathBuf::from("/tmp/whatever");
    let (temp, final_) = timestamped_wav_paths(&dir);
    assert!(temp
      .to_string_lossy()
      .starts_with("/tmp/whatever/recording-"));
    assert!(temp.to_string_lossy().ends_with(".wav.tmp"));
    assert!(final_.to_string_lossy().ends_with(".wav"));
    assert!(!final_.to_string_lossy().ends_with(".wav.tmp"));
  }

  #[test]
  fn finalize_produces_a_real_playable_wav_file_with_correct_metadata() {
    let dir = std::env::temp_dir().join("pr5_writer_test_finalize");
    std::fs::create_dir_all(&dir).unwrap();
    let temp_path = dir.join("rec.wav.tmp");
    let final_path = dir.join("rec.wav");
    let _ = std::fs::remove_file(&temp_path);
    let _ = std::fs::remove_file(&final_path);

    let format = AudioFormat {
      sample_rate: 48_000,
      channels: 2,
    };
    let spec = hound::WavSpec {
      channels: format.channels as u16,
      sample_rate: format.sample_rate,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let writer = hound::WavWriter::create(&temp_path, spec).unwrap();
    let (sender, receiver) = mpsc::channel::<WriterMessage>();

    // run_writer_loop needs a real AppHandle, which a plain unit test can't
    // construct. Its `app`/`state` parameters are only touched on the
    // write-error path, which this happy-path test never exercises -- so we
    // call the same finalize logic directly here rather than through
    // run_writer_loop, to keep this test AppHandle-free. The exact
    // write/flush/finalize/rename sequence matches run_writer_loop's
    // Finalize arm above verbatim. `temp_path`/`final_path` are cloned
    // before the move so the outer test can still assert on them afterward.
    let thread_temp_path = temp_path.clone();
    let thread_final_path = final_path.clone();
    let handle = thread::spawn(move || {
      let mut writer = writer;
      loop {
        match receiver.recv().unwrap() {
          WriterMessage::Frame(samples) => {
            for s in samples {
              writer.write_sample(s).unwrap();
            }
            writer.flush().unwrap();
          }
          WriterMessage::Finalize => {
            let duration_samples = writer.duration();
            let duration_ms = (duration_samples as u64 * 1000) / format.sample_rate.max(1) as u64;
            writer.finalize().unwrap();
            let size_bytes = std::fs::metadata(&thread_temp_path).unwrap().len();
            std::fs::rename(&thread_temp_path, &thread_final_path).unwrap();
            return Some(WriterResult {
              file_path: thread_final_path.to_string_lossy().to_string(),
              duration_ms,
              size_bytes,
            });
          }
          WriterMessage::Discard => return None,
        }
      }
    });

    sender.send(WriterMessage::Frame(vec![1, 2, 3, 4])).unwrap();
    sender.send(WriterMessage::Frame(vec![5, 6, 7, 8])).unwrap();
    sender.send(WriterMessage::Finalize).unwrap();

    let result = handle.join().unwrap().unwrap();
    assert_eq!(result.size_bytes, 44 + 8 * 2);
    assert_eq!(result.duration_ms, (4 * 1000) / 48_000);
    assert!(!std::fs::exists(&temp_path).unwrap());
    assert!(std::fs::exists(&final_path).unwrap());

    let reader = hound::WavReader::open(&final_path).unwrap();
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.spec().sample_rate, 48_000);
    let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
    assert_eq!(samples, vec![1, 2, 3, 4, 5, 6, 7, 8]);

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn discard_deletes_the_temp_file_without_producing_a_result() {
    let dir = std::env::temp_dir().join("pr5_writer_test_discard");
    std::fs::create_dir_all(&dir).unwrap();
    let temp_path = dir.join("rec.wav.tmp");
    let _ = std::fs::remove_file(&temp_path);

    let spec = hound::WavSpec {
      channels: 1,
      sample_rate: 48_000,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let writer = hound::WavWriter::create(&temp_path, spec).unwrap();
    let (sender, receiver) = mpsc::channel::<WriterMessage>();

    let thread_temp_path = temp_path.clone();
    let handle = thread::spawn(move || -> Option<WriterResult> {
      let mut writer = writer;
      loop {
        match receiver.recv().unwrap() {
          WriterMessage::Frame(samples) => {
            for s in samples {
              writer.write_sample(s).unwrap();
            }
          }
          WriterMessage::Discard => {
            drop(writer);
            std::fs::remove_file(&thread_temp_path).unwrap();
            return None;
          }
          WriterMessage::Finalize => panic!("test only sends Discard"),
        }
      }
    });

    sender.send(WriterMessage::Frame(vec![9, 9])).unwrap();
    sender.send(WriterMessage::Discard).unwrap();
    let result = handle.join().unwrap();

    assert!(result.is_none());
    assert!(!std::fs::exists(&temp_path).unwrap());
    std::fs::remove_dir_all(&dir).ok();
  }
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib writer::`
Expected: PASS (3 tests).

- [ ] **Step 5: Full verification**

Run, from `apps/sound-app/src-tauri`: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.
Expected: all clean. (`writer.rs`'s public items will show `dead_code` warnings under `cargo clippy` from the plain non-test lib target, since nothing outside this module and its own tests calls them yet — Task 3 wires it into `commands.rs`/`lib.rs`. If you see these warnings, this is expected and Task 3 resolves it; do not add a suppressing attribute in this task.)

Actually — check this precisely before moving on: run `cargo clippy --all-targets` and note the exact warning count/items. If `writer.rs` shows dead-code warnings, that is fine and matches the pattern from a prior PR's Task 2 (a capture backend that similarly wasn't wired into `lib.rs` until its own later task) — no fix needed in this task, Task 3 will make everything reachable.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/state.rs src/writer.rs
git commit -m "feat(sound-app): add guarded transitions and a WAV writer subsystem"
```

---

### Task 2: Orphaned-temp-file recovery

**Files:**
- Create: `apps/sound-app/src-tauri/src/recovery.rs`

**Interfaces:**
- Consumes: nothing from Task 1 directly (pure filesystem + `hound`), but shares the same `.wav.tmp` naming convention `writer::timestamped_wav_paths` produces.
- Produces (used by Task 3): `pub fn recover_orphaned_recordings(dir: &std::path::Path) -> std::io::Result<Vec<std::path::PathBuf>>`.

- [ ] **Step 1: Create `recovery.rs`**

```rust
use std::path::{Path, PathBuf};

/// Recomputes and overwrites a WAV file's two size fields (RIFF chunk size
/// at byte offset 4, data chunk size at byte offset 40) from its actual
/// byte count on disk. This is a fallback for a temp file that crashed
/// before its first `hound` `flush()` call ever completed (the placeholder
/// header `hound::WavWriter::create` writes has size fields of 0, which
/// don't match however many raw sample bytes actually made it to disk).
/// Only correct for the plain 44-byte PCM16, <=2-channel header `hound`
/// writes for this app's recordings (never the 64-byte WAVEFORMATEXTENSIBLE
/// header, which `hound` only uses above 2 channels or 16 bits).
fn patch_header(path: &Path) -> std::io::Result<()> {
  let total_size = std::fs::metadata(path)?.len();
  if total_size < 44 {
    return Err(std::io::Error::new(
      std::io::ErrorKind::InvalidData,
      "smaller than a WAV header",
    ));
  }
  let data_len = (total_size - 44) as u32;
  let riff_len = (total_size - 8) as u32;

  use std::io::{Seek, SeekFrom, Write};
  let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
  f.seek(SeekFrom::Start(4))?;
  f.write_all(&riff_len.to_le_bytes())?;
  f.seek(SeekFrom::Start(40))?;
  f.write_all(&data_len.to_le_bytes())?;
  f.flush()?;
  Ok(())
}

/// Recovers one orphaned `*.wav.tmp` file: promotes it to a final, playable
/// `.wav` (same name, `.tmp` suffix stripped) if it has any audio data,
/// deletes it otherwise. Returns the final path if promoted, `None` if
/// deleted.
fn recover_one(tmp_path: &Path) -> std::io::Result<Option<PathBuf>> {
  let already_valid = hound::WavReader::open(tmp_path)
    .map(|r| r.len() > 0)
    .unwrap_or(false);

  if !already_valid {
    // Try the manual patch fallback (a crash before the first flush).
    let _ = patch_header(tmp_path);
  }

  let has_data = hound::WavReader::open(tmp_path)
    .map(|r| r.len() > 0)
    .unwrap_or(false);

  if !has_data {
    std::fs::remove_file(tmp_path)?;
    return Ok(None);
  }

  let final_path = tmp_path.with_extension(""); // strips the trailing ".tmp"
  std::fs::rename(tmp_path, &final_path)?;
  Ok(Some(final_path))
}

/// Scans `dir` for `*.wav.tmp` files left behind by an interrupted recording
/// (a real capture/write error, a force-quit, or a crash) and recovers each
/// one. Meant to run once, synchronously, at app startup before the window
/// opens — fast, since it's at most a directory listing plus a couple of
/// small stray files in the overwhelmingly common case.
pub fn recover_orphaned_recordings(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
  let mut recovered = Vec::new();
  if !dir.exists() {
    return Ok(recovered);
  }
  for entry in std::fs::read_dir(dir)? {
    let entry = entry?;
    let path = entry.path();
    let is_temp_wav = path.extension().and_then(|e| e.to_str()) == Some("tmp")
      && path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.ends_with(".wav"))
        .unwrap_or(false);
    if is_temp_wav {
      if let Some(final_path) = recover_one(&path)? {
        recovered.push(final_path);
      }
    }
  }
  Ok(recovered)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_flushed_temp(dir: &Path, name: &str, samples: &[i16]) -> PathBuf {
    let path = dir.join(name);
    let spec = hound::WavSpec {
      channels: 2,
      sample_rate: 48_000,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for &s in samples {
      writer.write_sample(s).unwrap();
    }
    writer.flush().unwrap();
    std::mem::forget(writer); // simulate a crash right after the last flush
    path
  }

  fn make_unflushed_spill_temp(dir: &Path, name: &str, samples: &[i16]) -> PathBuf {
    let path = dir.join(name);
    let spec = hound::WavSpec {
      channels: 2,
      sample_rate: 48_000,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let file = std::fs::File::create(&path).unwrap();
    let small_buf = std::io::BufWriter::with_capacity(64, file); // forces a spill quickly
    let mut writer = hound::WavWriter::new(small_buf, spec).unwrap();
    for &s in samples {
      writer.write_sample(s).unwrap();
    }
    std::mem::forget(writer); // crash before any explicit flush
    path
  }

  #[test]
  fn recovers_a_cleanly_flushed_orphan_by_just_renaming() {
    let dir = std::env::temp_dir().join("pr5_recovery_test_flushed");
    std::fs::create_dir_all(&dir).unwrap();
    let tmp = make_flushed_temp(&dir, "recording-a.wav.tmp", &[1, 2, 3, 4]);

    let recovered = recover_orphaned_recordings(&dir).unwrap();
    assert_eq!(recovered.len(), 1);
    assert!(recovered[0].to_string_lossy().ends_with("recording-a.wav"));
    assert!(!std::fs::exists(&tmp).unwrap());

    let reader = hound::WavReader::open(&recovered[0]).unwrap();
    let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
    assert_eq!(samples, vec![1, 2, 3, 4]);
    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn recovers_a_never_flushed_spill_orphan_via_manual_patch() {
    let dir = std::env::temp_dir().join("pr5_recovery_test_spill");
    std::fs::create_dir_all(&dir).unwrap();
    let big_signal: Vec<i16> = (0..2000).collect();
    let tmp = make_unflushed_spill_temp(&dir, "recording-b.wav.tmp", &big_signal);

    let recovered = recover_orphaned_recordings(&dir).unwrap();
    assert_eq!(recovered.len(), 1);
    assert!(!std::fs::exists(&tmp).unwrap());

    let reader = hound::WavReader::open(&recovered[0]).unwrap();
    assert!(reader.len() > 0);
    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn deletes_a_temp_file_with_no_audio_data() {
    let dir = std::env::temp_dir().join("pr5_recovery_test_empty");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("recording-c.wav.tmp");
    let spec = hound::WavSpec {
      channels: 2,
      sample_rate: 48_000,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let writer = hound::WavWriter::create(&path, spec).unwrap();
    writer.finalize().unwrap(); // zero samples, but a valid header

    let recovered = recover_orphaned_recordings(&dir).unwrap();
    assert_eq!(recovered.len(), 0);
    assert!(!std::fs::exists(&path).unwrap());
    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn ignores_non_tmp_files_and_missing_directories() {
    let dir = std::env::temp_dir().join("pr5_recovery_test_missing");
    let _ = std::fs::remove_dir_all(&dir);
    let recovered = recover_orphaned_recordings(&dir).unwrap();
    assert!(recovered.is_empty());
  }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib recovery::`
Expected: PASS (4 tests).

- [ ] **Step 3: Full verification**

Run: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.
Expected: all clean (same expected dead-code note as Task 1's Step 5 — `recovery.rs`'s public function isn't called outside its own tests until Task 3).

- [ ] **Step 4: Commit**

```bash
git add src/recovery.rs
git commit -m "feat(sound-app): recover orphaned temp recordings on startup"
```

---

### Task 3: Wire the writer and recovery into production

**Files:**
- Modify: `apps/sound-app/src-tauri/src/state.rs` (add `writer` field to `SharedState`)
- Modify: `apps/sound-app/src-tauri/src/commands.rs` (`try_start`, `make_frame_callback`, `stop_recording`, `cancel_recording`)
- Modify: `apps/sound-app/src-tauri/src/lib.rs` (register modules, run recovery in `.setup()`)

**Interfaces:**
- Consumes: everything from Tasks 1-2 (`writer::*`, `recovery::recover_orphaned_recordings`, `state::try_transition`).

- [ ] **Step 1: Add `SharedState.writer`**

In `apps/sound-app/src-tauri/src/state.rs`, change the `SharedState` struct and its `new`:

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
```

- [ ] **Step 2: Update `commands.rs`'s imports and `try_start`**

Change the import block at the top of `apps/sound-app/src-tauri/src/commands.rs` from:

```rust
use crate::capture::{AudioFormat, AudioSource, FrameCallback};
use crate::state::{CommandError, RecordingState, SharedState};
use crate::tick::{buffer_duration_ms, compute_level};
```

to:

```rust
use crate::capture::{AudioFormat, AudioSource, FrameCallback};
use crate::state::{try_transition, CommandError, RecordingState, SharedState};
use crate::tick::{buffer_duration_ms, compute_level};
use crate::writer::{self, WriterHandle, WriterMessage};
```

Replace the `try_start` function body:

```rust
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
```

- [ ] **Step 3: Update `make_frame_callback`**

Replace the doc comment and function:

```rust
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
```

(`buffer` is moved into the `Frame` message only as the very last use, after every earlier step that needed it borrowed `&buffer` — no clone needed.)

- [ ] **Step 4: Reorder and guard `stop_recording`**

Replace the whole function:

```rust
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
```

- [ ] **Step 5: Reorder and guard `cancel_recording`**

Replace the whole function:

```rust
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
```

- [ ] **Step 6: Wire the new modules and startup recovery into `lib.rs`**

Replace the full file:

```rust
mod capture;
mod commands;
mod recovery;
mod state;
mod tick;
mod writer;

use capture::linux_pulse::LinuxPulseCapture;
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
      match writer::recording_dir(app.handle()) {
        Ok(dir) => {
          if let Err(e) = recovery::recover_orphaned_recordings(&dir) {
            log::warn!("Could not recover orphaned recordings: {e}");
          }
        }
        Err(e) => log::warn!("Could not resolve save directory for recovery: {e}"),
      }
      Ok(())
    })
    .manage(SharedState::new(Box::new(LinuxPulseCapture::new())))
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

(A failed orphan recovery, or a failed save-directory lookup, logs a warning and lets the app start normally — it must never block app startup over a stray leftover file.)

- [ ] **Step 7: Full automated verification**

Run: `cargo test`
Expected: PASS. All prior tests plus Tasks 1-2's new tests.

Run: `cargo clippy --all-targets`
Expected: zero warnings now (Tasks 1-2's previously-expected dead-code warnings are resolved, since `writer`/`recovery` are now reachable from `lib.rs`).

Run: `cargo fmt --check`
Expected: no diff.

- [ ] **Step 8: Manual verification**

Run: `pnpm --filter sound-app tauri dev`

In the launched app:
1. Record real audio (play music), stop, and note the reported duration/size in the `Saved` state (visible via the app's existing UI — check the console/devtools if the UI doesn't surface `size_bytes` directly yet).
2. Open the resulting file (in `~/Music/Sound Recorder/` or your platform's audio directory) with a media player (or `aplay`/`ffplay`) and confirm it plays back the real audio that was recorded, not silence or garbage.
3. Record again, then Cancel mid-recording — confirm no `.wav` or `.wav.tmp` file is left behind in the save directory.
4. Record again, then forcibly kill the app process (e.g. `pkill -9 -f target/debug/sound-app`) while it's actively recording. Relaunch `pnpm --filter sound-app tauri dev` and confirm a recovered `.wav` file appears in the save directory (not a `.tmp`) and plays back the audio captured before the kill.

This exercises the full lifecycle this PR adds — passing automated tests alone don't confirm the file is real, playable audio.

- [ ] **Step 9: Commit**

```bash
git add src/state.rs src/commands.rs src/lib.rs
git commit -m "feat(sound-app): wire the WAV writer and orphan recovery into production"
```

---

## Verification (whole plan)

```bash
cd apps/sound-app/src-tauri
cargo test               # all tests pass
cargo clippy --all-targets  # zero warnings
cargo fmt --check         # no diff
cd ../../..
pnpm --filter sound-app tauri dev   # manual click-through + crash-recovery check per Task 3 Step 8
```

## Explicitly out of scope for this PR

- Disk-space pre-checks before/during recording (PR 6: storage checks).
- Any UI for browsing, playing back, renaming, or deleting recordings (PR 7).
- A configurable save location or folder picker (PR 8) — this PR's save directory is a fixed default.
- Non-WAV output formats.
- Full guarded-transition hardening of `start_recording`/`pause_recording`/`resume_recording`.
- Windows/macOS.
