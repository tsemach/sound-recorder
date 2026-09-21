# sound-app: Real Linux Capture (PR 4 of 8)

Status: approved for planning
Scope: `apps/sound-app` only. Depends on PR 3 (recording state machine, merged to master).

## Context

PR 3 built the entire recording state machine, command surface, and UI
against `FakeCapture` — a synthetic sine-wave generator, not real audio.
This PR swaps `FakeCapture` for `LinuxPulseCapture`, a real implementation
of the `AudioCapture` trait that captures actual system audio via
PulseAudio/PipeWire's monitor sources. By the end of this PR, the app
records the audio actually playing on the machine — but doesn't yet write
it to a file (PR 5).

Key PRD constraints this PR must honor:
- §6.2: capture system output audio; detect available output devices and
  allow source selection.
- §8: isolate platform-specific capture behind a common recording interface
  (the `AudioCapture` trait, already built in PR 3).
- Non-goal (§3): microphone recording — only monitor (loopback) sources are
  offered, real microphone inputs are filtered out.

### PR 3's carryover gaps — resolved by this design, not deferred further

PR 3's final review found the `AudioCapture` trait as originally defined
couldn't actually absorb real capture unchanged (documented in
`docs/superpowers/specs/2026-09-20-sound-app-recording-state-machine-design.md`'s
"Deviations from this spec" section). Both are resolved here:

1. **No async error channel.** `start()` could only report failures
   synchronously at call time — nothing let a background capture thread
   report a failure that happens *after* `start()` already returned `Ok`
   (device unplugged, daemon restart, buffer underrun). This is also why
   `RecordingState::Error` had no producer in the codebase before this PR.
2. **No audio format information.** `tick.rs` hardcoded 48kHz mono to match
   `FakeCapture`'s exact output. Real capture needed to either force a
   fixed format (with PulseAudio resampling) or discover and use the
   source's actual native format.

### Verified against this machine's real audio stack, not guessed

Before finalizing this design, the exact `libpulse-binding`/
`libpulse-simple-binding` API surface (versions 2.30.1/2.29.0) was
prototyped and run against this machine's real PipeWire/Pulse-compatible
daemon:

- Source enumeration via the **standard (non-threaded) `Mainloop`** +
  `Context` + `context.introspect().get_source_info_list(...)`, driven by a
  simple `iterate(true)` loop — no background thread needed for this
  one-off call. Confirmed working: correctly found and distinguished the
  real monitor source (`alsa_output.pci-0000_00_1f.3.analog-stereo.monitor`,
  `monitor_of_sink: Some(_)`) from the real microphone input
  (`alsa_input.pci-0000_00_1f.3.analog-stereo`, `monitor_of_sink: None`).
- **The source's real format is 48kHz/stereo**, not the mono originally
  assumed during PR 3's planning — confirmed via `SourceInfo.sample_spec`.
  This directly informed the format-handling decision below.
- Real streaming capture via `libpulse-simple-binding`'s blocking `Simple`
  API (`Direction::Record`), reading from the actual monitor source,
  succeeded with zero errors (10 chunks of real audio data read cleanly;
  `Simple` is confirmed `Send`, so it can be moved into a background
  thread).
- No new system packages were required — `libpulse-sys` linked against the
  system's existing PulseAudio client library without any `sudo dnf
  install` (already present via PipeWire's Pulse-compatibility layer,
  likely pulled in transitively by earlier GTK/WebKit dependencies).

### Decisions made during design

- **Hybrid API approach**: the non-threaded standard `Mainloop` for the
  infrequent `list_sources()` call, the blocking `Simple` API for the
  continuous streaming read loop. Avoids the far more complex threaded
  `Mainloop` + async `Stream` API entirely — that API exists for
  applications needing concurrent multi-stream/event-driven access, which
  this app doesn't.
- **Use the source's real native format, not a forced fixed target.**
  `list_sources()`'s introspection query already returns each source's
  actual `sample_rate`/`channels`; `LinuxPulseCapture` uses those exact
  values when opening the `Simple` stream, avoiding any implicit
  PulseAudio-side resampling and preserving native quality.
- **`FrameCallback` now delivers `Result<Vec<i16>, CaptureError>`, not a
  bare `Vec<i16>`.** A real capture failure mid-stream calls the callback
  once with `Err(...)`, then the thread exits. `SharedState.state` widens
  from `Mutex<RecordingState>` to `Arc<Mutex<RecordingState>>` (matching
  the existing pattern for `elapsed_ms`/`level`/`last_tick_emit`) so the
  frame callback — which already holds an `AppHandle` clone — can
  transition `RecordingState` to `Error{recoverable: true}` and emit it
  directly from the background thread, without needing capture code to
  know anything about Tauri.
- **New `AudioCapture::format() -> AudioFormat` trait method**, valid after
  a successful `start()`. `tick.rs`'s `buffer_duration_ms` becomes
  format-aware instead of hardcoding a sample-rate constant.
  `FakeCapture::format()` keeps returning its existing 48kHz/mono constant
  — no behavior change to any already-passing PR 3 test.
- **Pause/resume needs no PulseAudio-level cork/uncork.** The capture
  thread keeps calling `.read()` continuously even while paused (so
  PulseAudio's internal buffer doesn't back up), but discards the data
  instead of invoking the frame callback — identical semantics to
  `FakeCapture`'s existing pause behavior.
- **No async commands.** Both source enumeration and stream setup are
  fast, local, blocking calls — no network/disk latency. This means PR 3's
  check-then-act race analysis (safe only because Tauri dispatches sync
  commands inline, never concurrently) **remains valid and unchanged** —
  that deferred item stays deferred, this PR doesn't reopen it.
- **Testing**: pure logic (byte→i16 conversion, the monitor-source filter
  predicate, format-aware `tick.rs` math) gets real unit tests. Given this
  repo currently has no CI, real-daemon integration tests are ALSO added
  (per explicit choice) that connect to this machine's actual PipeWire/Pulse
  daemon — but written to degrade gracefully (warn and return early,
  not fail) if no daemon is reachable, so they don't become a fragile
  landmine if CI or a different contributor's machine is added later.

## Architecture

### Module structure (`apps/sound-app/src-tauri/src/`)

```
capture/
  mod.rs         — AudioCapture trait (widened), AudioFormat (new),
                   FrameCallback (widened to Result<Vec<i16>, CaptureError>)
  fake.rs        — FakeCapture, updated to the new trait shape (no behavior change)
  linux_pulse.rs — LinuxPulseCapture (new)
tick.rs          — buffer_duration_ms becomes format-aware
state.rs         — SharedState.state: Mutex<RecordingState> -> Arc<Mutex<RecordingState>>
commands.rs      — make_frame_callback updated for the Result-wrapped callback
                   and the new state Arc; lib.rs swaps FakeCapture for LinuxPulseCapture
```

### Widened trait

```rust
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

`AudioSource` is unchanged (`{ id, name }`) — format info flows through
`format()`, not through the source list, since it's only needed once a
source is actually selected and started.

### `LinuxPulseCapture`

```rust
pub struct LinuxPulseCapture {
    running: Arc<AtomicBool>,
    paused: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
    format: Arc<Mutex<AudioFormat>>,
}
```

`list_sources()`: standard `Mainloop` + `Context`, connect, iterate to
`Ready`, `get_source_info_list` filtered to `monitor_of_sink.is_some()`,
mapped to `AudioSource { id: name, name: description.unwrap_or(name) }`.

`start(source_id, on_frame)`:
1. Run the same enumeration internally to find the matching source's
   `sample_spec` (rate + channels) — reuses the `list_sources` logic, one
   extra local round-trip, negligible cost for a user-initiated action.
2. Store the discovered format in `self.format` (for later `format()` calls).
3. Synchronously create `Simple::new(None, "Sound Recorder", Direction::Record, Some(source_id), "Recording", &spec, None, None)` — connection failures return here, before any thread is spawned.
4. Spawn a thread that owns the `Simple` and loops: `.read(&mut buf)` (buffer sized for ~20ms at the discovered rate/channels) → on success, convert bytes to `Vec<i16>` via `i16::from_ne_bytes` pairs, call `on_frame(Ok(samples))` (or silently discard if paused); on failure, call `on_frame(Err(...))` once and exit the loop.

`format()`: returns the last-discovered format (a sane default before any
`start()` call, though nothing reads it before then).

`pause()`/`resume()`: same `Arc<AtomicBool>` toggle as `FakeCapture`.

`stop()`: sets the running flag false, joins the thread. `Simple` drops
inside the thread closure, closing the connection via its own `Drop` impl.

### Frame callback error handling (`commands.rs`)

`make_frame_callback` gains a `state: Arc<Mutex<RecordingState>>` parameter
(alongside the existing `Arc`-wrapped `elapsed_ms`/`level`/`last_tick_emit`).
On `Ok(samples)`, behavior is unchanged (update elapsed/level, throttled
tick emission). On `Err(e)`, the callback directly sets `*state.lock() =
RecordingState::Error { message: e.message, recoverable: true }` and emits
`recording-state-changed` — the first real producer of the `Error` state,
retiring the last `#[allow(dead_code)]` attribute in the codebase.

## Testing strategy

- `tick.rs`: extend existing tests for the format-aware `buffer_duration_ms`
  signature (now taking channels + sample rate, not a hardcoded constant).
- New pure-function unit tests: byte-pair → `i16` conversion, the
  monitor-source filter predicate (pulled out as a standalone testable
  function operating on already-fetched `SourceInfo`-shaped data, not
  requiring a live connection).
- Real-daemon integration tests (in `linux_pulse.rs`): a `list_sources()`
  test and a short real-capture test connecting to this machine's actual
  PipeWire/Pulse daemon. Each attempts connection with a bounded number of
  iterations; on failure to reach `Ready`, prints a warning and returns
  early (does not fail the test) — real assertions only run when a daemon
  is actually reachable, which it is on this machine.
- Manual: `pnpm --filter sound-app tauri dev` — confirm the source dropdown
  shows the real monitor source (not `FakeCapture`'s fake entries), and
  that recording responds to actual audio (e.g. playing something and
  watching the level meter move, versus silence keeping it flat).

## Explicitly out of scope for this PR

- Real file writing (PR 5's WAV writer) — `stop_recording` still emits a
  fake `file_path`/`size_bytes` in `Saved`, now with a real `duration_ms`.
- Windows/macOS capture implementations.
- Storage/disk-space checks (PR 6).
- The check-then-act race across commands — confirmed still deferred,
  correctly, since nothing in this PR requires async commands.
- Any change to `RecordingState`'s shape, transition predicates, or the
  command/event surface itself — this PR only swaps the capture
  implementation and widens the trait it implements.
