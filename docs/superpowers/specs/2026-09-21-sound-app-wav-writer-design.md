# sound-app: WAV Writer + Atomic Finalize (PR 5 of 8)

Status: approved for planning
Scope: `apps/sound-app` only. Depends on PR 4 (real Linux capture, merged to master).

## Context

PR 4 made the app capture real system audio, but `stop_recording` still emits a
hardcoded fake `file_path`/`size_bytes` — no file is ever written. This PR
makes that real: PCM frames arriving from `AudioCapture` are written
incrementally to a temp WAV file as they arrive, and on a clean stop the file
is finalized (correct header) and atomically renamed to its final name.

Key PRD constraints this PR must honor:
- §6.1/§8: write audio incrementally, no full-recording-in-memory; unique
  timestamped filenames; use temp files + atomic rename to prevent corrupt
  visible files; recover/clean up temp files after crashes.
- §8: preserve correct sample rate, channels, duration, and container
  metadata.
- §10: preserve a partial recording rather than silently discarding it, when
  possible.

### Carryover context from PR 4's final review

PR 4's final whole-branch review (before its SDD workspace was deleted;
preserved in git history at commits `7a73b39`/`9dc708c`) flagged four things
PR 5 must account for:

1. **Disk I/O must not run on the capture thread.** The frame callback
   currently runs on the same thread that calls the blocking
   `libpulse_simple_binding::Simple::read()` in a loop. A synchronous write
   there risks a PulseAudio buffer overrun if it stalls past one buffer
   period (~20ms). This PR hands frames to a dedicated writer thread over a
   channel instead of writing inline.
2. **No cleanup hook exists for the `Error` state.** `can_stop()`/
   `can_cancel()` are both `false` for `Error`, so a real mid-stream capture
   failure leaves the capture's `self.handle` and (once this PR exists) an
   in-progress temp file with no finalize-or-discard path.
3. **Nothing stops capture or finalizes a temp file on app exit.** No `Drop`
   impl, no window-close handler. This PR is what first makes a temp file
   real, so this gap becomes concrete rather than hypothetical.
4. **`AudioCapture::format()` (not `tick::SAMPLE_RATE_HZ`) must be the WAV
   header's source of truth** for sample rate/channel count — the constant
   is now dead code outside `FakeCapture`'s own fixed test format.

A fifth, closely related item the same review named as **"a PR 5
prerequisite, not fixed [in PR 4]"**: `stop_recording` currently reads state,
then unconditionally writes `Saving` → `Saved` with no re-check — a capture
error landing concurrently can be silently overwritten by a fake success.
This was cosmetic in PR 4 (the file was fake anyway) and stops being
cosmetic here, since `Saved`'s `file_path` becomes real. This spec addresses
it (see "Guarded transitions" below).

### Decisions made during design

- **Default save location**: `<OS audio directory>/Sound Recorder/` (e.g.
  `~/Music/Sound Recorder` on this machine, via Tauri's
  `app.path().audio_dir()`), created on first use if missing. No settings
  screen exists yet (PR 8); this is the sane default a user would expect to
  find recordings in, and PR 8's folder picker later just changes it.
- **Filename**: `recording-YYYY-MM-DD_HH-MM-SS.wav` (unique, timestamped,
  per §6.1). The in-progress file is the same name with `.tmp` appended:
  `recording-YYYY-MM-DD_HH-MM-SS.wav.tmp`, in the same folder.
- **`hound` crate for WAV I/O**, not a hand-rolled writer. WAV header/chunk
  math is a classic footgun (wrong byte offsets, endianness), and the PRD
  explicitly requires correct container metadata — a well-tested crate
  removes that risk for the normal (non-orphaned) write path.
- **Unified orphaned-temp-file recovery**, covering both the `Error`-path
  and app-exit gaps with one mechanism instead of two bespoke handlers: on
  startup, scan for `*.wav.tmp` files and finalize each into a real,
  playable recording (or delete it, if it has zero audio data). Both gaps
  produce the identical artifact — a temp file that was never cleanly
  finalized — so one recovery pass covers both, rather than building a
  live in-process error handler for one and a window-close handler for the
  other.
- **Guarded transitions, scoped to `stop_recording`/`cancel_recording`
  only.** These are the only two commands whose unconditional writes could
  silently report a fake success for a real failure (the actual harm the
  reviewer flagged). `pause_recording`/`resume_recording`/`start_recording`
  are left as-is — losing their narrower race merely delays a state flip by
  one tick, not misreport a failed recording as saved. Full test-and-set
  hardening of all five commands is not this PR's job.

  **Correction (recorded after implementation, by the final whole-branch
  review):** this "merely delays a state flip" claim does not fully hold —
  `try_start` now does real directory/file creation before returning, and a
  writer or capture error racing that window can still reach
  `stop_recording` later and, absent Fix 3's header-only guard, would have
  reported a fake `Saved` for zero real audio data. Fix 3 (checking
  `size_bytes > 44` before reporting `Saved`) closes the concrete harm;
  full guarded-transition coverage of `start`/`pause`/`resume` remains
  recommended future work, not done in this PR.
- **Rejected approaches**: writing to disk inline in the existing frame
  callback (explicitly warned against by PR 4's review — real xrun risk);
  an async/Tokio-based writer requiring `async` Tauri commands (would
  reopen the check-then-act race across *all* commands that PR 3/PR 4
  deliberately kept closed by staying fully sync — a much bigger blast
  radius for no benefit here, since a plain OS thread + channel already
  solves the actual problem).

## Architecture

### Module structure (`apps/sound-app/src-tauri/src/`)

```
writer.rs      — WavRecorder: spawns the writer thread, WriterMessage enum,
                 WriterHandle (sender + join handle), WriterResult
recovery.rs    — orphaned-temp-file scan + header-patch reconstruction,
                 run once from lib.rs's .setup() before the window opens
state.rs       — SharedState gains `writer: Mutex<Option<WriterHandle>>`;
                 new `try_transition` guarded-transition helper
commands.rs    — try_start spawns the writer alongside capture; the frame
                 callback forwards frames to the writer's channel instead of
                 only computing tick/level; stop_recording/cancel_recording
                 reordered to call capture.stop() first, then guard their
                 state transition before finalizing/discarding the writer
lib.rs         — calls recovery::recover_orphaned_recordings() in .setup()
```

### Writer subsystem (`writer.rs`)

```rust
pub enum WriterMessage {
    Frame(Vec<i16>),
    Finalize,
    Discard,
}

pub struct WriterResult {
    pub file_path: String,
    pub duration_ms: u64,
    pub size_bytes: u64,
}

pub struct WriterHandle {
    pub sender: std::sync::mpsc::Sender<WriterMessage>,
    pub join_handle: std::thread::JoinHandle<Result<WriterResult, CaptureError>>,
}
```

`spawn_writer(temp_path: PathBuf, final_path: PathBuf, format: AudioFormat, app: AppHandle, state: Arc<Mutex<RecordingState>>) -> WriterHandle`
creates the `hound::WavWriter` at `temp_path` with a spec derived from
`format` (`bits_per_sample: 16`, `sample_format: Int`), spawns a thread that
loops on `receiver.recv()`:

- `Frame(samples)`: write each sample via the `hound` writer. On a write
  error (e.g. disk full), set `RecordingState::Error` directly (same
  pattern the capture thread already uses) and stop processing further
  frames — but keep looping to drain (and discard) any further messages
  until `Finalize`/`Discard`/channel-close, so the thread doesn't leave a
  dangling sender-side block.
- `Finalize`: call the `hound` writer's finalize, compute the real file size
  via `fs::metadata`, atomically rename `temp_path` → `final_path`, return
  `Ok(WriterResult { .. })`.
- `Discard`: drop the `hound` writer (no finalize needed), delete
  `temp_path`, return an `Ok` result with empty/zero fields (the caller
  ignores these for the `Idle` transition `cancel_recording` produces).
- Channel closed with no terminal message received (the sender was only
  held by the frame callback, and the capture thread exited due to an
  error without an explicit terminal send): drop the `hound` writer without
  finalizing and exit the thread. This deliberately leaves an
  **unfinalized** temp file on disk — the orphan-recovery scan (below)
  picks it up on next startup. No command is waiting on this thread's
  `join_handle` in this scenario, so nothing blocks.

The frame callback (`make_frame_callback` in `commands.rs`) gets a clone of
the `Sender<WriterMessage>` and does `let _ = sender.send(WriterMessage::Frame(samples.clone()))`
(cheap, non-blocking on an unbounded channel) alongside its existing
elapsed/level bookkeeping — audio still reaches the writer even if this
send races the writer thread hitting an error and starting to drain.

### Guarded transitions (`state.rs`)

```rust
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

`stop_recording`/`cancel_recording` reorder to: check the fast-fail guard
(unchanged), call `capture.stop()` **first** (its `.join()` guarantees any
concurrent capture-thread error's state-write already landed — thread join
is a happens-before edge), then call `try_transition` to move to
`Saving`/`Idle`. If it returns `false` (state changed underneath —
overwhelmingly likely to be a concurrent `Error`), the command backs off:
does not touch the writer, does not emit anything further, returns `Ok(())`
(the state machine has already correctly reflected what happened; there is
nothing left for this command to do).

### Orphan recovery (`recovery.rs`)

Run once, synchronously, from `lib.rs`'s `.setup()` hook before the window
opens (fast — a directory listing plus, at most, a couple of small stray
files):

1. List `*.wav.tmp` in the save directory.
2. For each: read the file's total byte size via `fs::metadata`. If it's
   `<= 44` bytes (the fixed canonical PCM16 WAV header size — no audio data
   was ever written), delete it.
3. Otherwise, compute `data_len = total_size - 44` and patch two 4-byte
   little-endian fields directly in the file at their fixed canonical
   offsets: the `RIFF` chunk size (offset 4, `= total_size - 8`) and the
   `data` sub-chunk size (offset 40, `= data_len`) — the same correction a
   clean `hound` finalize performs, applied after the fact to a file whose
   writer never got to run it.
4. Rename the patched file to its final name (strip the `.tmp` suffix).

The exact byte-patch code will be validated for real (write, interrupt,
patch, reopen with `hound::WavReader`, confirm correct sample count) during
plan-writing, matching this project's established practice of validating
Rust code by actually compiling and running it before it goes into a plan.

### Command integration (`commands.rs`)

`try_start` (called from `start_recording`) already computes the real
`AudioFormat` from `capture.format()` right after a successful
`capture.start()`. At that same point, it now also:
- builds the temp/final paths (helper in `writer.rs` or inline: save
  directory + timestamped filename + `.tmp` suffix),
- calls `writer::spawn_writer(...)`,
- stores the returned `WriterHandle` in `state.writer.lock().unwrap()`.

`make_frame_callback` gains a `Sender<WriterMessage>` parameter and forwards
every `Ok(samples)` frame to it (before or after the existing elapsed/level
math — order doesn't matter, they're independent).

`stop_recording`: after the reordering above, on a successful
`try_transition` to `Saving`: take the `WriterHandle` out of
`state.writer`, send `Finalize`, join, and use the real `WriterResult` to
emit `Saved { file_path, duration_ms, size_bytes }` (replacing today's
hardcoded `"fake-recording.wav"`/`0`). If `capture.stop()` itself returned
an error, keep the current behavior (revert to `previous_state`, return
`Err`) — but the writer must still be signaled to `Discard` in that case
(a failed capture stop with no clean data isn't worth keeping as a
"successful" save); the SDD plan will spell out this exact branch.

`cancel_recording`: same reordering; on success, take the `WriterHandle`,
send `Discard`, join (ignoring the result), emit `Idle`.

## Testing strategy

- **Pure logic**: the header byte-patch math (given a total file size,
  compute the correct `RIFF`/`data` size field values) as a standalone
  function, unit-tested without touching the filesystem.
- **Writer integration**: spawn a real writer against a temp directory
  (`tempfile` crate or `std::env::temp_dir()` + a unique subpath), send a
  few `Frame`s, send `Finalize`, join, then reopen the finalized file with
  `hound::WavReader` and assert the sample count/format/duration match
  what was sent.
- **Discard path**: same setup, send `Discard`, assert the temp file no
  longer exists on disk.
- **Guarded transition**: unit tests for `try_transition` covering both the
  "allowed, transitions" and "not allowed, leaves state untouched" cases.
- **Orphan recovery**: write a temp file that mimics an interrupted
  recording (a valid 44-byte placeholder header + N raw PCM bytes, no
  finalize), run the recovery function directly against a temp directory,
  assert the recovered file opens correctly via `hound::WavReader` with the
  correct sample count, and the original `.tmp` file is gone. Also test the
  zero-data-case: a `.tmp` file with nothing beyond the header gets deleted,
  not promoted.
- **Manual**: `pnpm --filter sound-app tauri dev` — record real audio
  (music playing), stop, and actually play back the resulting `.wav` file
  (e.g. via a media player or `aplay`) to confirm it contains the real
  audio, not silence or garbage. Also manually verify Cancel leaves no
  file behind, and (if feasible) kill the app mid-recording and confirm the
  next launch recovers a playable partial file.

## Explicitly out of scope for this PR

- Disk-space pre-checks before/during recording (PR 6: storage checks).
- Any UI for browsing, playing back, renaming, or deleting recordings
  (PR 7: recordings list screen).
- A configurable save location or folder picker (PR 8: settings screen) —
  this PR's save directory is a fixed default.
- Non-WAV output formats (Opus, mentioned as an MVP option in the PRD) —
  WAV only, per the earlier PR 1 design decision.
- Full guarded-transition hardening of `start_recording`/`pause_recording`/
  `resume_recording` — scoped to `stop_recording`/`cancel_recording` only,
  per the rationale above.
- Windows/macOS.
