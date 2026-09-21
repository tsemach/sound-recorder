# sound-app: Storage Checks + Error Surfacing (PR 6 of 8)

Status: approved for planning
Scope: `apps/sound-app` only. Depends on PR 5 (WAV writer + atomic finalize, merged to master).

## Context

The recording pipeline is now real end-to-end (PR4's capture, PR5's writer),
but nothing checks whether there's actually disk space to write to, and one
lingering concurrency gap from PR5's review (unguarded `start`/`pause`/
`resume`) becomes more consequential once this PR adds a new, more likely
source of mid-recording errors.

Key PRD/spec requirements this PR must honor:
- §6.1: "Validate available storage before and during recording."
- §10: "insufficient storage" listed among conditions needing actionable
  errors; "preserve a partial recording rather than silently discarding it."
- Overall migration spec (PR 6 roadmap entry): pre-flight and in-flight
  disk-space checks; `Error` state wired to an actual UI error banner.

### What's already done, not rediscovered

PR4's review found the frontend never surfaced `RecordingState::Error`'s
message; PR5's fix wave closed that gap (`useRecordingState.ts` now calls
`setError(message)` on `Error`, `App.tsx` renders it in a banner). That part
of "Error UI wiring" is done. What's still open is narrower: `Saving` has no
UI indication at all, and this PR's own new disk-space error messages need
to actually be worded actionably (the mechanism already exists; only the
new error paths using it are new).

### Carryover context from PR5's final review

1. **The writer thread's frame channel is unbounded** (`std::sync::mpsc::channel`).
   A struggling disk could grow it without limit, in tension with CLAUDE.md's
   "no full-recording-in-memory" constraint — resolved here (see Decisions).
2. **`Saving` has no UI affordance or escape hatch.** If `stop_recording`'s
   `writer.join()` blocks on a stalling disk, the user sees nothing. This PR
   adds a status indicator, not an interrupt mechanism (see Decisions).
3. **`try_transition` is scoped to `stop`/`cancel` only.** PR5's review found
   the rationale for excluding `start`/`pause`/`resume` ("merely delays a
   state flip") no longer fully holds once `Saved` carries real data. This
   PR extends it to all five commands (see Decisions).
4. **No app-exit cleanup, no capture-thread `stop()` timeout.** Both remain
   explicitly out of scope for this PR — neither is disk-space-related, and
   widening this PR to cover them risks scope creep into unrelated
   concurrency work.

### Decisions made during design

- **`fs4` for disk-space queries** — a small, actively-maintained crate
  focused purely on filesystem space/lock queries (a fork of the older
  `fs2`), wrapping `statvfs` on Linux. Matches this project's established
  minimal-dependency bar (the same reasoning that chose `libpulse-binding`
  over a higher-level audio crate, and evaluated `hound` against a
  hand-rolled writer rather than reaching for a heavier crate by default).
- **Fixed 200MB threshold**, a `const` in the Rust code. No settings UI
  exists yet (PR 8); making this configurable is explicitly deferred there.
- **Pre-flight check in `try_start`**, before `capture.start()` is called at
  all — query free space on the save directory's filesystem; below the
  threshold, reject with an actionable `CommandError`, no state change
  (the guard fails exactly like the existing "unknown source" check).
- **In-flight check runs on the writer thread**, not the capture thread's
  frame callback. PR4's review established that the frame callback must
  stay free of disk-adjacent work (risk of PulseAudio buffer overruns) —
  the writer thread already does all the disk I/O, so it's the natural
  place for this, and it already has the temp file's path to resolve the
  right filesystem. Throttled to once every 5 seconds (checked opportunistically
  whenever a `Frame` message arrives — cheap enough not to need its own
  timer thread), matching the "periodically" language in the PRD without
  adding a syscall per ~20ms frame. Below the threshold: same treatment as
  a write failure — set `RecordingState::Error{recoverable:true}` and keep
  draining (not writing) further frames, exactly mirroring the existing
  write-failure `failed` latch in `run_writer_loop`.
- **Bounded writer channel + `try_send`, not blocking `send`.** Switches
  `mpsc::channel` (unbounded) to `mpsc::sync_channel` with a generous
  capacity (250 messages ≈ 5 seconds of buffered audio at typical frame
  sizes) — caps memory growth, closing PR5's carryover gap. Critically, the
  frame callback uses `try_send`, not `send`: a blocking send on a full
  channel would stall the capture thread waiting for the writer to drain,
  reintroducing exactly the capture-thread-disk-coupling hazard PR4's
  review already eliminated. On `TrySendError::Full`, the frame is dropped
  (not the whole recording) — but a full channel is itself a strong signal
  the writer can't keep up, so it also triggers the same `Error` transition
  a write failure does, rather than silently dropping audio indefinitely.
- **Guarded transitions extended to all five commands.** `start_recording`/
  `pause_recording`/`resume_recording`'s final state-write becomes a
  `try_transition` call guarded by that command's own `can_*` predicate,
  exactly mirroring `stop_recording`/`cancel_recording`'s existing pattern.
  No change to any command's happy-path behavior — this only changes what
  happens when a concurrent error lands in the same narrow window a command
  is already mid-call.
- **`Saving` UI**: a simple "Saving…" label shown while `state.state ===
  "Saving"` — no new buttons, no cancel/interrupt mechanism. Addresses the
  "user sees nothing happening" gap without taking on the harder, separate
  problem of interrupting a blocking `join()`.

## Architecture

### Module structure (`apps/sound-app/src-tauri/src/`)

```
storage.rs     — disk-space query (fs4-backed) + the shared threshold
                 constant + the pure "below threshold" check
writer.rs      — sync_channel (bounded) instead of channel; run_writer_loop
                 gains a throttled in-flight disk check on each Frame,
                 reusing storage.rs's check
commands.rs    — try_start gates on a pre-flight storage.rs check before
                 capture.start(); frame callback uses try_send + Full
                 triggers Error; start/pause/resume/stop/cancel all use
                 try_transition
```

### `storage.rs`

```rust
pub const MIN_FREE_BYTES: u64 = 200 * 1024 * 1024; // 200 MB

/// Queries free space on the filesystem containing `path` (a file or
/// directory; the file need not exist yet — only its parent directory
/// does, matching how `try_start` calls this before the temp file exists).
pub fn free_space_bytes(path: &std::path::Path) -> std::io::Result<u64> {
  fs4::available_space(path)
}

/// Pure predicate, directly unit-testable without touching a real
/// filesystem.
pub fn is_below_threshold(free_bytes: u64) -> bool {
  free_bytes < MIN_FREE_BYTES
}
```

### Pre-flight (`commands.rs`'s `try_start`)

Before `capture.start()` is called (the very first check in the function,
alongside the existing source-lookup logic): resolve the save directory
(`writer::recording_dir(app)`, already computed here for the temp/final
paths — this check reuses that same call rather than duplicating it), query
`storage::free_space_bytes`, and if `storage::is_below_threshold(...)`,
return `Err(CommandError::new("Not enough free disk space to start recording (need at least 200MB free)"))`
without touching capture or the writer at all.

### In-flight (`writer.rs`'s `run_writer_loop`)

A new `last_storage_check: Instant` (initialized at loop start) is checked
on each `Ok(WriterMessage::Frame(...))` arrival, throttled the same way
`commands.rs`'s existing tick emission is throttled: if 5 seconds have
elapsed since the last check, re-check `storage::free_space_bytes` against
the temp file's directory; if below threshold, set the `failed` latch (the
same one the write-failure path already sets) with message "Recording
stopped: disk space is critically low" and transition to `Error` — the
existing `failed`-latch machinery then makes every subsequent `Frame`
message a no-op drain until `Finalize`/`Discard`/channel-close, exactly as
it already does for a write failure.

### Bounded channel + `try_send` (`writer.rs`, `commands.rs`)

`writer::create_channel()` changes from `mpsc::channel()` to
`mpsc::sync_channel(250)`. `commands.rs`'s frame callback changes its send
call from `let _ = writer_sender.send(WriterMessage::Frame(buffer));` to:

```rust
match writer_sender.try_send(WriterMessage::Frame(buffer)) {
  Ok(()) => {}
  Err(std::sync::mpsc::TrySendError::Full(_)) => {
    // The writer can't keep up. Drop this one frame rather than block the
    // capture thread (which would reintroduce the exact capture/disk
    // coupling PR 4's review eliminated) -- but treat a full channel as a
    // real failure signal, same as a disk-space or write error.
    *state.lock().unwrap() = RecordingState::Error {
      message: "Recording stopped: the audio writer fell behind".to_string(),
      recoverable: true,
    };
    let _ = app.emit(
      "recording-state-changed",
      RecordingState::Error {
        message: "Recording stopped: the audio writer fell behind".to_string(),
        recoverable: true,
      },
    );
  }
  Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
    // Writer thread already exited (e.g. its own error path) -- nothing
    // to do, the state was already set by whatever caused that exit.
  }
}
```

(Exact code will be validated for real during plan-writing, matching this
project's established practice — the shape above is illustrative of the
three-arm match, not yet compiler-checked.)

### Guarded transitions everywhere (`commands.rs`)

`start_recording`, `pause_recording`, `resume_recording` each replace their
final `emit_state(&app, &state, next)` call with the same pattern
`stop_recording`/`cancel_recording` already use:

```rust
if try_transition(&state.state, RecordingState::can_pause, next.clone()) {
  let _ = app.emit("recording-state-changed", next);
}
```

(substituting each command's own `can_*` predicate and target state). If
the transition doesn't happen (state moved concurrently), the command
silently returns without emitting — the concurrent event's own emission
already told the frontend what actually happened.

### Frontend (`App.tsx`)

One new conditional render: a `state.state === "Saving"` branch showing a
"Saving…" label, using the same conditional-render pattern already used for
the error banner and the active-recording controls. No new hook state, no
new events — `Saving` is already a value `RecordingState` can be.

## Testing strategy

- **Pure logic**: `storage::is_below_threshold` unit tests (below, at, and
  above the threshold).
- **Real disk-space query**: a test calling `storage::free_space_bytes` on
  a real temp directory, asserting it returns a plausible (nonzero) value —
  degrades gracefully (skip with a warning, matching this project's
  established pattern for hardware/environment-dependent tests) if `fs4`
  can't resolve the filesystem in the test environment.
- **Bounded channel / `try_send`**: a test that fills a small-capacity test
  channel and confirms `try_send` returns `Full` rather than blocking, and
  that the frame-callback logic correctly triggers `Error` on that path
  (following the same "duplicate the exact logic inline" testing pattern
  already established for `run_writer_loop`, since the callback isn't
  independently testable without a real `AppHandle`).
- **Guarded transitions**: extend `state.rs`'s existing `try_transition`
  test coverage with cases for `can_pause`/`can_resume`/`can_start`
  predicates (mirroring the existing `can_stop` tests).
- **Manual**: `pnpm --filter sound-app tauri dev` — verify the pre-flight
  rejection and in-flight interruption using a temporarily-lowered
  threshold constant (or a small loopback/tmpfs filesystem sized to trigger
  it) so the real 200MB default doesn't need to be manufactured on a real
  disk; verify the `Saving…` indicator appears during a normal stop; verify
  no behavior change in the happy-path record/pause/resume/stop/cancel flow
  after the guarded-transition extension.

## Explicitly out of scope for this PR

- Making the 200MB threshold configurable (PR 8: settings screen).
- Interrupting a stuck `Saving` (blocking `writer.join()`) — this PR only
  adds a status indicator, not an escape hatch; a real fix needs its own
  concurrency design pass.
- `LinuxPulseCapture::stop()`'s potential hang if the underlying blocking
  `Simple::read()` stalls without erroring — a capture-side concern
  unrelated to disk space, still deferred from PR 4's review.
- App-exit cleanup (no `Drop`/window-close handler stopping capture) — still
  deferred from PR 4's review, unrelated to storage.
- Any UI beyond the `Saving…` label and the already-existing error banner —
  no new recordings-list or settings UI (PR 7, PR 8).
- Windows/macOS.
