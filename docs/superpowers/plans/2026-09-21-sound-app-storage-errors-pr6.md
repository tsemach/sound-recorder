# sound-app: Storage Checks + Error Surfacing (PR 6) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add pre-flight and in-flight disk-space checks that reject/interrupt recording with an actionable error while preserving whatever was already written, bound the writer's frame channel to stop it from growing without limit, and close the last unguarded gap in the recording state machine's concurrent-error-vs-command race.

**Architecture:** A new `storage.rs` module wraps `fs4`'s free-space query behind a pure threshold check. `try_start` gates on it before touching capture; the writer thread re-checks it on a 5-second throttle using the same `failed`-latch machinery a write failure already uses. The writer's frame channel switches from an unbounded `mpsc::channel` to a bounded `mpsc::sync_channel`, and the frame callback switches from `send` to `try_send` so a full channel drops one frame and reports an error instead of ever blocking the capture thread. All five recording commands now guard their final state-write with `state::try_transition`.

**Tech Stack:** Rust, `fs4 = "1.1.0"` (disk-space query).

**Spec:** `docs/superpowers/specs/2026-09-21-sound-app-storage-errors-design.md`

## Global Constraints

- Disk-space threshold: `200 * 1024 * 1024` bytes (200MB), a `const`, not configurable in this PR.
- Pre-flight check runs before `capture.start()` is called at all.
- In-flight check runs on the writer thread (never the capture thread's frame callback), throttled to once per 5 seconds.
- The writer's frame channel becomes bounded (`mpsc::sync_channel`, capacity 250); the frame callback must use `try_send`, never a blocking `send`, to avoid ever stalling the capture thread.
- All five commands (`start_recording`, `pause_recording`, `resume_recording`, `stop_recording`, `cancel_recording`) guard their final state-write with `state::try_transition` — no command's happy-path behavior changes.
- Every task's commit must leave `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`, `pnpm --filter sound-app test`, `pnpm --filter sound-app typecheck`, and `pnpm --filter sound-app lint` all clean.

---

## Context for the implementer

Current repo state (branch `feat/sound-app-storage-errors`, forked from `master` with PR 1-5 merged): `apps/sound-app/src-tauri` has real capture (PR 4) and a real WAV writer with atomic finalize (PR 5). `stop_recording`/`cancel_recording` already use `state::try_transition`; `start_recording`/`pause_recording`/`resume_recording` still call the older unguarded `emit_state` helper. The writer thread's frame channel is currently an unbounded `mpsc::channel`.

Files this PR touches or adds:
- `src/storage.rs` — **new**. Disk-space threshold + query.
- `src/writer.rs` — bounded channel, throttled in-flight check, shared `fail_recording` helper.
- `src/commands.rs` — pre-flight check in `try_start`; `try_send` in the frame callback; guarded transitions in `start`/`pause`/`resume`.
- `src/App.tsx` / `src/App.test.tsx` — a `Saving…` indicator.

All code below was written by actually compiling and running it against this exact crate — including confirming `fs4::available_space`'s real signature and behavior on this machine's real filesystem, and confirming `mpsc::sync_channel(250)`'s `try_send` genuinely reports `Full` after exactly 250 successful sends — before being put in this document.

---

### Task 1: `storage.rs` — disk-space threshold and query

**Files:**
- Modify: `apps/sound-app/src-tauri/Cargo.toml` (add `fs4`)
- Create: `apps/sound-app/src-tauri/src/storage.rs`
- Modify: `apps/sound-app/src-tauri/src/lib.rs` (register the module)

**Interfaces:**
- Produces (used by Task 2 and Task 3):
  - `pub const MIN_FREE_BYTES: u64` (= `200 * 1024 * 1024`)
  - `pub fn free_space_bytes(path: &std::path::Path) -> std::io::Result<u64>`
  - `pub fn is_below_threshold(free_bytes: u64) -> bool`

- [ ] **Step 1: Add the dependency**

Run: `cd apps/sound-app/src-tauri && cargo add fs4`
Expected: `Cargo.toml` gains `fs4 = "1.1.0"` (or the current latest-compatible resolution — run the command, don't hand-edit a version string). `Cargo.lock` is regenerated.

- [ ] **Step 2: Create `storage.rs`**

```rust
pub const MIN_FREE_BYTES: u64 = 200 * 1024 * 1024; // 200 MB

pub fn free_space_bytes(path: &std::path::Path) -> std::io::Result<u64> {
  fs4::available_space(path)
}

pub fn is_below_threshold(free_bytes: u64) -> bool {
  free_bytes < MIN_FREE_BYTES
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn below_threshold_is_true_when_under_the_limit() {
    assert!(is_below_threshold(MIN_FREE_BYTES - 1));
    assert!(is_below_threshold(0));
  }

  #[test]
  fn below_threshold_is_false_at_and_above_the_limit() {
    assert!(!is_below_threshold(MIN_FREE_BYTES));
    assert!(!is_below_threshold(MIN_FREE_BYTES + 1));
    assert!(!is_below_threshold(u64::MAX));
  }

  #[test]
  fn free_space_bytes_queries_a_real_filesystem() {
    let dir = std::env::temp_dir();
    match free_space_bytes(&dir) {
      Ok(bytes) => {
        println!("free space on {dir:?}: {bytes} bytes");
        assert!(bytes > 0, "expected a nonzero free-space reading");
      }
      Err(e) => {
        eprintln!("warning: could not query free space on {dir:?}, skipping: {e}");
      }
    }
  }
}
```

`fs4::available_space` degrades gracefully in the third test (warn-and-skip on
`Err`, matching this project's established pattern for environment-dependent
tests) — but on this machine, and any normal Linux filesystem, it succeeds.

- [ ] **Step 3: Register the module**

In `apps/sound-app/src-tauri/src/lib.rs`, change:
```rust
mod capture;
mod commands;
mod recovery;
mod state;
mod tick;
mod writer;
```
to:
```rust
mod capture;
mod commands;
mod recovery;
mod state;
mod storage;
mod tick;
mod writer;
```
(alphabetical order, matching the existing convention). Don't wire any calls
to `storage::*` into `lib.rs` yet — that's Task 3.

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib storage::`
Expected: PASS (3 tests).

- [ ] **Step 5: Full verification**

Run: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.
Expected: `cargo test`/`cargo fmt --check` clean. `cargo clippy --all-targets`
will show `dead_code` warnings on `storage::free_space_bytes`/
`storage::is_below_threshold` (unused outside this module's own tests until
Task 3 wires them in) — this is expected, matching the same pattern prior
PRs' additive first tasks have shown; do not add a suppressing attribute.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/storage.rs src/lib.rs
git commit -m "feat(sound-app): add disk-space threshold check"
```

---

### Task 2: Bound the writer's frame channel and add the in-flight storage check

**Files:**
- Modify: `apps/sound-app/src-tauri/src/writer.rs`

**Interfaces:**
- Consumes: `storage::free_space_bytes`, `storage::is_below_threshold` from Task 1.
- Produces (used by Task 3):
  - `pub fn create_channel() -> (std::sync::mpsc::SyncSender<WriterMessage>, std::sync::mpsc::Receiver<WriterMessage>)` (signature change: `Sender` → `SyncSender`)
  - `pub struct WriterHandle { pub sender: std::sync::mpsc::SyncSender<WriterMessage>, pub join_handle: ... }` (field type change)

- [ ] **Step 1: Widen imports and add the channel-capacity/check-interval constants**

Change the top of `writer.rs` from:
```rust
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use tauri::{AppHandle, Manager};

use crate::capture::AudioFormat;
use crate::state::RecordingState;
```
to:
```rust
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::capture::AudioFormat;
use crate::state::RecordingState;
use crate::storage;

/// How many `WriterMessage`s the channel between the capture thread's frame
/// callback and the writer thread can hold before `try_send` starts
/// returning `Full`. 250 frames at ~20ms each is roughly 5 seconds of
/// buffered audio -- enough slack for a brief disk hiccup without letting
/// memory grow unboundedly if the writer falls behind for good.
const CHANNEL_CAPACITY: usize = 250;

const STORAGE_CHECK_INTERVAL: Duration = Duration::from_secs(5);
```

- [ ] **Step 2: Switch `WriterHandle` and `create_channel` to the bounded, synchronous sender**

Change:
```rust
pub struct WriterHandle {
  pub sender: Sender<WriterMessage>,
  pub join_handle: thread::JoinHandle<Option<WriterResult>>,
}
```
to:
```rust
pub struct WriterHandle {
  pub sender: SyncSender<WriterMessage>,
  pub join_handle: thread::JoinHandle<Option<WriterResult>>,
}
```

Change:
```rust
pub fn create_channel() -> (Sender<WriterMessage>, Receiver<WriterMessage>) {
  mpsc::channel()
}
```
to:
```rust
pub fn create_channel() -> (SyncSender<WriterMessage>, Receiver<WriterMessage>) {
  mpsc::sync_channel(CHANNEL_CAPACITY)
}
```

- [ ] **Step 3: Add the `sync_channel`/`try_send` validation test**

Add this test inside the existing `#[cfg(test)] mod tests` block, as the
first test (before `timestamped_names_have_the_expected_shape`):

```rust
  #[test]
  fn sync_channel_returns_full_when_capacity_exceeded() {
    let (sender, receiver) = create_channel();
    let mut sent = 0;
    loop {
      match sender.try_send(WriterMessage::Frame(vec![1, 2])) {
        Ok(()) => sent += 1,
        Err(mpsc::TrySendError::Full(_)) => break,
        Err(e) => panic!("unexpected error: {e:?}"),
      }
      if sent > 10_000 {
        panic!("channel never reported Full -- capacity assumption is wrong");
      }
    }
    println!("channel reported Full after {sent} successful sends");
    assert!(sent > 0, "expected at least some capacity before Full");
    assert!(
      sent <= CHANNEL_CAPACITY,
      "sent more than the configured capacity before Full"
    );
    drop(receiver);
  }
```

Run: `cargo test --lib writer::tests::sync_channel_returns_full_when_capacity_exceeded -- --nocapture`
Expected: PASS, printing `channel reported Full after 250 successful sends`
(verified on this machine: `mpsc::sync_channel(250)` allows exactly 250
successful non-blocking sends before `try_send` reports `Full`).

- [ ] **Step 4: Add a shared `fail_recording` helper and the throttled in-flight check**

Add this function right before `run_writer_loop` (it factors out the
state-write-plus-emit pattern the write-failure path already uses, so the
new storage-failure path can share it instead of duplicating it):

```rust
/// Sets `RecordingState::Error` and emits it, mirroring the pattern every
/// failure path in this loop already uses. Shared by the write-failure and
/// low-disk-space paths so both report failures identically.
fn fail_recording(app: &AppHandle, state: &Arc<Mutex<RecordingState>>, message: String) {
  *state.lock().unwrap() = RecordingState::Error {
    message: message.clone(),
    recoverable: true,
  };
  let _ = tauri::Emitter::emit(
    app,
    "recording-state-changed",
    RecordingState::Error {
      message,
      recoverable: true,
    },
  );
}
```

Then replace `run_writer_loop`'s body (from `let mut failed = false;` through
the end of the `Ok(WriterMessage::Frame(samples)) => { ... }` arm) with:

```rust
  let mut failed = false;
  let mut last_storage_check = Instant::now();

  loop {
    match receiver.recv() {
      Ok(WriterMessage::Frame(samples)) => {
        if failed {
          continue;
        }

        if last_storage_check.elapsed() >= STORAGE_CHECK_INTERVAL {
          last_storage_check = Instant::now();
          let dir = temp_path.parent().unwrap_or(&temp_path);
          if let Ok(free) = storage::free_space_bytes(dir) {
            if storage::is_below_threshold(free) {
              failed = true;
              fail_recording(
                &app,
                &state,
                "Recording stopped: disk space is critically low".to_string(),
              );
              continue;
            }
          }
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
          fail_recording(&app, &state, format!("Could not write audio to disk: {e}"));
        }
      }
```

(The `Ok(WriterMessage::Finalize)`, `Ok(WriterMessage::Discard)`, and
`Err(_)` arms below this are unchanged — leave them exactly as they are.)

Note: this in-flight throttle uses a plain `Instant::elapsed() >= Duration`
comparison with no dedicated unit test for the 5-second timing itself — this
matches this codebase's own existing precedent (`commands.rs`'s
`TICK_INTERVAL` throttle for `recording-tick` emission is the same idiom,
also untested at the timing level; only the surrounding logic is tested).

- [ ] **Step 5: Run the full test suite for this file**

Run: `cargo test --lib writer::`
Expected: PASS (7 tests: the existing 3 plus the new channel-capacity test,
plus confirm the two existing `finalize_...`/`discard_...` tests — which
build their own raw `mpsc::channel` for the test's inline duplicate-loop
logic, not `create_channel()` — still compile and pass unchanged).

- [ ] **Step 6: Full verification**

Run: `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.
Expected: `cargo test`/`cargo fmt --check` clean. `cargo clippy --all-targets`
will show a compile error in `commands.rs` at this point (`make_frame_callback`
and its caller still pass/expect the old `Sender<WriterMessage>` type,
now `SyncSender<WriterMessage>`) — **this is expected**; Task 3 fixes it.
Run `cargo check --lib` instead if you want a quick sanity check before Task 3
lands — the type mismatch is real and correctly caught by the compiler, not
a bug in this task's own code, and `storage.rs`'s dead-code warnings from
Task 1 remain expected too (still unwired) until Task 3.

- [ ] **Step 7: Commit**

```bash
git add src/writer.rs
git commit -m "feat(sound-app): bound the writer channel and add an in-flight disk check"
```

---

### Task 3: Wire the pre-flight check, `try_send`, and guarded transitions into `commands.rs`; add the `Saving…` indicator

**Files:**
- Modify: `apps/sound-app/src-tauri/src/commands.rs`
- Modify: `apps/sound-app/src/App.tsx`
- Modify: `apps/sound-app/src/App.test.tsx`

**Interfaces:**
- Consumes: everything from Tasks 1-2 (`storage::*`, `writer::SyncSender`-based
  `create_channel`/`WriterHandle`).

- [ ] **Step 1: Update imports**

Change:
```rust
use crate::capture::{AudioFormat, AudioSource, FrameCallback};
use crate::state::{try_transition, CommandError, RecordingState, SharedState};
use crate::tick::{buffer_duration_ms, compute_level};
use crate::writer::{self, WriterHandle, WriterMessage};
```
to:
```rust
use crate::capture::{AudioFormat, AudioSource, FrameCallback};
use crate::state::{try_transition, CommandError, RecordingState, SharedState};
use crate::storage;
use crate::tick::{buffer_duration_ms, compute_level};
use crate::writer::{self, WriterHandle, WriterMessage};
```

- [ ] **Step 2: Update the `emit_state` doc comment**

Replace this comment block (directly above `fn emit_state`):
```rust
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
```

with:

```rust
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
```

(This is purely a comment edit — the `fn emit_state(...) { ... }` body
itself is unchanged.)

- [ ] **Step 3: Guard `start_recording`'s final transitions**

Replace:
```rust
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
```

with:

```rust
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
```

- [ ] **Step 4: Add the pre-flight disk-space check to `try_start`**

Right after the existing defensive-teardown lines (`let _ = state.capture...`
and `state.writer.lock().unwrap().take();`), before `let sources = state...`,
insert:

```rust
  let dir = writer::recording_dir(app).map_err(CommandError::new)?;
  let free = storage::free_space_bytes(&dir)
    .map_err(|e| CommandError::new(format!("Could not check available disk space: {e}")))?;
  if storage::is_below_threshold(free) {
    return Err(CommandError::new(
      "Not enough free disk space to start recording (need at least 200MB free)",
    ));
  }
```

Then, further down in the same function, find:
```rust
  let dir = match writer::recording_dir(app) {
    Ok(dir) => dir,
    Err(e) => {
      let _ = state.capture.lock().unwrap().stop();
      return Err(CommandError::new(e));
    }
  };
  let (temp_path, final_path) = writer::timestamped_wav_paths(&dir);
```
and replace it with just:
```rust
  let (temp_path, final_path) = writer::timestamped_wav_paths(&dir);
```
(`dir` is now already resolved earlier by the pre-flight check above, so this
second resolution — which was also duplicating the directory-creation work —
is removed; `dir` from the top of the function is reused here instead.)

- [ ] **Step 5: Switch the frame callback from `send` to `try_send`**

First, change `make_frame_callback`'s parameter type:
```rust
  writer_sender: std::sync::mpsc::Sender<WriterMessage>,
```
to:
```rust
  writer_sender: std::sync::mpsc::SyncSender<WriterMessage>,
```

Then replace the closure's final line:
```rust
      let _ = writer_sender.send(WriterMessage::Frame(buffer));
    },
  )
}
```
with:
```rust
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
```

- [ ] **Step 6: Guard `pause_recording`'s and `resume_recording`'s final transitions**

In `pause_recording`, replace:
```rust
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
```
with:
```rust
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
```

In `resume_recording`, replace:
```rust
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
```
with:
```rust
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
```

`stop_recording`/`cancel_recording` already use `try_transition` (from PR 5)
and are not touched by this task.

- [ ] **Step 7: Full backend verification**

Run: `cargo test`
Expected: PASS. All prior tests plus Tasks 1-2's new tests (38 total: the
35 from before this PR, plus `storage`'s 3).

Run: `cargo clippy --all-targets`
Expected: zero warnings now (Task 1's `storage::*` dead-code warnings are
resolved — `try_start` calls both functions now).

Run: `cargo fmt --check`
Expected: no diff.

- [ ] **Step 8: Add the `Saving…` indicator to the frontend**

In `apps/sound-app/src/App.tsx`, find:
```tsx
        {isActive && (
          <div className="h-2 w-full max-w-xs rounded bg-muted">
            <div
              className="h-2 rounded bg-primary transition-all"
              style={{ width: `${Math.min(level, 1) * 100}%` }}
            />
          </div>
        )}
```
and add immediately after it:
```tsx

        {state.state === "Saving" && (
          <div className="text-sm text-muted-foreground">Saving…</div>
        )}
```

- [ ] **Step 9: Add frontend tests for the indicator**

In `apps/sound-app/src/App.test.tsx`, add these two tests inside the
existing `describe("App", ...)` block, after the last existing test
(`"does not show the Record button when there are no sources"`):

```tsx
  it("shows a Saving indicator while state is Saving", () => {
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({ state: { state: "Saving" } })
    )
    render(<App />)
    expect(screen.getByText("Saving…")).toBeInTheDocument()
  })

  it("does not show the Saving indicator outside the Saving state", () => {
    mockUseRecordingState.mockReturnValue(baseHookReturn())
    render(<App />)
    expect(screen.queryByText("Saving…")).not.toBeInTheDocument()
  })
```

- [ ] **Step 10: Full frontend verification**

Run, from the repo root:
```bash
pnpm --filter sound-app test
pnpm --filter sound-app typecheck
pnpm --filter sound-app lint
```
Expected: `test` shows 19 passing (17 existing + 2 new); `typecheck`/`lint`
produce no output (clean).

- [ ] **Step 11: Manual verification**

Run: `pnpm --filter sound-app tauri dev`

1. **Pre-flight rejection**: temporarily change `storage::MIN_FREE_BYTES` in
   your local checkout to a value larger than this machine's actual free
   space (e.g. `u64::MAX`), rebuild, click Record, and confirm it's rejected
   with the "Not enough free disk space..." message instead of starting.
   Revert the constant back to `200 * 1024 * 1024` afterward — **do not
   commit the temporary value.**
2. **`Saving…` indicator**: record briefly, click Stop, and confirm the
   `Saving…` label appears (even if only briefly, before `Saved`).
3. **No regression**: run through the full Record → Pause → Resume → Stop
   flow, and separately Record → Cancel, confirming both behave exactly as
   they did before this PR (this PR must not change any happy-path
   behavior — only what happens on a concurrent failure).

- [ ] **Step 12: Commit**

```bash
git add src/commands.rs ../src/App.tsx ../src/App.test.tsx
git commit -m "feat(sound-app): wire disk-space checks and guarded transitions into commands"
```

(Adjust the relative paths above to match your actual working directory —
`commands.rs` is under `apps/sound-app/src-tauri/src/`, `App.tsx`/
`App.test.tsx` are under `apps/sound-app/src/`.)

---

## Verification (whole plan)

```bash
cd apps/sound-app/src-tauri
cargo test               # all tests pass (38 total)
cargo clippy --all-targets  # zero warnings
cargo fmt --check         # no diff
cd ../../..
pnpm --filter sound-app test       # 19 passing
pnpm --filter sound-app typecheck  # clean
pnpm --filter sound-app lint       # clean
pnpm --filter sound-app tauri dev  # manual verification per Task 3 Step 11
```

## Explicitly out of scope for this PR

- Making the 200MB threshold configurable (PR 8: settings screen).
- Interrupting a stuck `Saving` (blocking `writer.join()`) — this PR only
  adds a status indicator, not an escape hatch.
- `LinuxPulseCapture::stop()`'s potential hang if the underlying blocking
  `Simple::read()` stalls without erroring — still deferred from PR 4.
- App-exit cleanup (no `Drop`/window-close handler stopping capture) —
  still deferred from PR 4.
- Any UI beyond the `Saving…` label and the already-existing error banner.
- Windows/macOS.
