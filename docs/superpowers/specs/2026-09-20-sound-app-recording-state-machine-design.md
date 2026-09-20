# sound-app: Recording State Machine + Mocked Capture (PR 3 of 8)

Status: approved for planning
Scope: `apps/sound-app` only. Depends on PR 1 (Next.js → Vite) and PR 2 (Tauri shell), both merged to master.

## Context

PR 2 proved the Tauri IPC bridge works with a trivial `ping` command and a
throwaway button. This PR replaces that scaffolding entirely with the real
recording control flow: a Rust state machine, a full command/event surface,
and real frontend UI — backed by a **fake** capture source (no real audio
yet; that's PR 4's `libpulse-binding` work). By the end of this PR, a user
can click through Record → Pause → Resume → Stop and watch the whole state
machine work, with a fake elapsed timer and level meter, but no real audio
file gets written (PR 5 adds the WAV writer).

Key PRD constraints this PR must honor:
- §6.1: idle/preparing/recording/paused/saving/saved/error states; Pause/Resume
  and Stop shown "only when applicable"; confirm cancellation.
- §8: isolate platform-specific capture behind a common recording interface.
- §10: actionable errors, never silently discard state — commands must
  surface rejected/illegal actions to the UI, not fail silently.

### Scope correction from the overall migration spec

The overall migration spec's unified command list includes
`list_recordings`/`delete_recording`/`rename_recording`. Re-reading the PR
roadmap, those belong to **PR 7** ("Recordings list screen") — there's no
real file-writing backend until PR 5, and no UI to consume them until PR 7.
Implementing them now would operate on a save directory this PR never
actually writes files into. **This PR's command surface is recording-control
only**; the recordings-library commands are deferred to PR 7.

### Decisions made during design

- **Capture trait built now, not deferred to PR 4**: the `AudioCapture`
  trait (already sketched in the overall migration spec) is defined in this
  PR and `FakeCapture` implements it for real — not ad-hoc fake logic in the
  command handlers. PR 4 then only swaps in `LinuxPulseCapture` behind the
  same interface; the state machine, commands, and tick/level computation
  never change.
- **CSP fixed now**: `tauri.conf.json`'s `security.csp` (currently `null` —
  no policy at all, deferred from PR 2's review) gets set to a real
  restrictive policy in this PR, verified live against `tauri dev` to
  confirm Vite's dev-server/HMR still connects.
- **`cargo test` gets its own script**: `test:rust` (root + `apps/sound-app`),
  kept separate from the existing `test` (Vitest-only) so JS-only
  contributors aren't forced to install Rust just to run tests.
- **UI scope**: full recording UI — Record/Pause/Resume/Stop/Cancel buttons
  shown per current state, `mm:ss` elapsed time, and a plain fake level
  meter driven by `FakeCapture`'s synthetic signal (not flat/silent) — gives
  PR 4+ a real UI to plug real data into and makes the mocked backend's
  output visible.
- **No new shadcn components**: only `Button` exists in `packages/ui` so
  far. The source selector, level meter, and cancel-confirmation use plain
  HTML (`<select>`, a `<div>` bar, native `window.confirm()`) rather than
  adding `select`/`progress`/`alert-dialog` components in this PR — keeps
  scope tight, upgradeable in a later UI-polish pass.
- **The `ping` command and button (PR 2) are removed entirely** — they were
  explicit throwaway scaffolding proving the IPC bridge, superseded by this
  PR's real commands.

## Architecture

### Rust module structure (`apps/sound-app/src-tauri/src/`)

```
state.rs       — RecordingState enum + SharedState (Arc<Mutex<RecordingState>>)
                 + legal-transition validation
capture/
  mod.rs       — AudioCapture trait + AudioSource/CaptureError/FrameCallback types
  fake.rs      — FakeCapture: background thread generating a synthetic
                 sine-wave PCM stream
commands.rs    — #[tauri::command] functions, each Result<T, CommandError>
lib.rs         — wires managed state, registers commands (ping removed)
```

### State machine

```rust
enum RecordingState {
    Idle,
    Preparing,
    Recording { source_name: String, elapsed_ms: u64 },
    Paused { source_name: String, elapsed_ms: u64 },
    Saving,
    Saved { file_path: String, duration_ms: u64, size_bytes: u64 },
    Error { message: String, recoverable: bool },
}
```

`Recording`/`Paused` carry `elapsed_ms` as a snapshot (valid the moment a
transition happens, or if the frontend reconnects mid-session). The
continuously-updating value during active recording comes from
`recording-tick`, not repeated `state-changed` emissions — state changes
stay semantically meaningful (only real transitions fire) separate from the
high-frequency UI feed.

Since PR 5 hasn't landed yet, `Saved`'s `file_path`/`size_bytes` in this PR
are fake placeholder values (no real file is written) — the field shapes
stay as originally specified so PR 5 only needs to supply real values, not
change the type.

**Legal transitions** (everything else returns `Err(CommandError)`):
- `Idle | Saved | Error{recoverable:true}` --(`start_recording`)--> `Preparing` --> `Recording`
- `Recording` ⇄ `Paused` (`pause_recording` / `resume_recording`)
- `Recording | Paused` --(`stop_recording`)--> `Saving` --> `Saved`
- `Recording | Paused` --(`cancel_recording`)--> `Idle` (discarded, no `Saved`)

### Capture trait

```rust
trait AudioCapture: Send {
    fn list_sources(&self) -> Result<Vec<AudioSource>, CaptureError>;
    fn start(&mut self, source_id: &str, on_frame: FrameCallback) -> Result<(), CaptureError>;
    fn pause(&mut self);
    fn resume(&mut self);
    fn stop(&mut self) -> Result<(), CaptureError>;
}
```

`FakeCapture` implements this for real: `start()` spawns a background
thread generating a synthetic sine-wave PCM buffer (mono, fixed fake sample
rate) roughly every 20ms, calling `on_frame(bytes)` — the same shape real
capture uses in PR 4. The **command layer** (not the capture impl) tracks
frame count → `elapsed_ms` and computes RMS amplitude → `level` from each
buffer, throttling `recording-tick` emission to ~10Hz. PR 4 reuses this
tick/level computation unchanged — only `FakeCapture` gets swapped for
`LinuxPulseCapture`.

### Command surface

All commands take `tauri::State<SharedState>` (and `AppHandle` where an
event emit is needed) and return `Result<T, CommandError>` where
`CommandError { message: String, recoverable: bool }` (same shape as the
`Error` state variant, reused consistently):

- `list_sources() -> Vec<AudioSource>`
- `start_recording(source_id: String) -> ()`
- `pause_recording() -> ()`
- `resume_recording() -> ()`
- `stop_recording() -> ()`
- `cancel_recording() -> ()`

Illegal transitions (e.g. `pause_recording` while `Idle`) are rejected with
a structured error, not silently ignored or allowed to panic — the concrete
fix for PR 2's carryover finding that real commands must surface errors to
the UI (unlike the deleted `ping` button's silent-failure pattern).

### Events

- `recording-state-changed` — full `RecordingState` snapshot on every
  transition; the frontend's single source of truth.
- `recording-tick` — `{ elapsed_ms, level }`, ~10Hz, only while `Recording`.

### Frontend

`useRecordingState` hook (replaces the ping-button logic entirely):
listens for both events on mount, exposes
`{ state, elapsedMs, level, sources, error, startRecording, pauseRecording,
resumeRecording, stopRecording, cancelRecording }`. Every action wraps its
`invoke()` call in try/catch, setting a local `error` the UI renders.

`App.tsx` is replaced entirely (no more "Project ready!"/ping content):
- Source `<select>` (visible when `Idle`), populated from `list_sources()`.
- Record button: visible when `Idle | Saved | Error{recoverable:true}`.
- Pause/Resume + Stop + Cancel: visible only when `Recording`/`Paused`.
- Elapsed time formatted `mm:ss`.
- Level meter: a plain `<div>` bar tracking `level` (0–1) from `recording-tick`.
- Error banner rendering the hook's `error`.
- Cancel confirmation via native `window.confirm()` (PRD's "confirm
  cancellation" requirement, no new dialog component needed).

## Carryover fixes from PR 2's review

- **CSP**: `tauri.conf.json`'s `security.csp` → `"default-src 'self'; style-src 'self' 'unsafe-inline'"`.
  Verified live during implementation against `pnpm --filter sound-app tauri dev`
  to confirm Vite's dev-server/HMR still connects; if it breaks, the
  implementation plan documents the exact adjustment needed rather than
  guessing here.
- **Rust test wiring**: `"test:rust": "cd src-tauri && cargo test"` added to
  `apps/sound-app/package.json`, plus a root `"test:rust": "turbo test:rust"`
  script and matching Turbo task — kept separate from `pnpm test`.

## Testing strategy

- Rust: state-transition tests (every illegal transition from every state
  returns `Err`; every legal one succeeds and updates state correctly);
  `FakeCapture` produces frames at the expected cadence; tick throttling
  computes `elapsed_ms`/`level` correctly from a known synthetic input.
- Frontend: `useRecordingState` reacting correctly to a sequence of mocked
  `state-changed`/`tick` events (Vitest, mocking `listen`/`invoke` from
  `@tauri-apps/api`); component tests for button visibility per state.
- Manual: `pnpm --filter sound-app tauri dev` — full click-through (select
  source → record → pause → resume → stop → confirm `Saved`; also cancel
  mid-recording) plus the CSP dev/HMR check.

## Explicitly out of scope for this PR

- Real audio capture (PR 4), real file writing (PR 5).
- `list_recordings`/`delete_recording`/`rename_recording` commands and the
  recordings list screen (PR 7).
- New shadcn components (select/progress/alert-dialog) — plain HTML used
  instead for this pass.
- Storage/disk-space checks and the full `Error` UI banner styling beyond a
  plain error message (PR 6).
- Settings screen, save-location picker, filename templates (PR 8).

---

## Deviations from this spec (recorded after implementation)

- **The `AudioCapture` trait as defined here cannot actually absorb PR 4's real capture unchanged.** This spec's Architecture section claims "PR 4 then only swaps in `LinuxPulseCapture` behind the same interface; the state machine, commands, and tick/level computation never change." The final whole-branch review found this doesn't hold: `start()` can only report failures synchronously at call time, with no channel for the asynchronous failures real capture routinely produces (device unplugged, daemon restart, xrun) — which is also why `RecordingState::Error` has no producer yet. Real capture will need the trait widened (e.g. an error channel, or a `Result`-wrapped frame callback) — a trait signature change, not just a new implementation. This needs to be an explicit task in PR 4's planning, not discovered mid-implementation.
- **The capture interface carries no audio format, and `tick.rs` hardcodes 48kHz mono.** `FakeCapture` happens to produce exactly matching output, so the math in `buffer_duration_ms` is exact today — but real capture (commonly 44.1kHz stereo) would silently make the elapsed timer run at the wrong rate with no error, since nothing currently carries sample rate or channel count across the `AudioCapture` interface. PR 4 needs to decide where audio format information lives (e.g. `AudioCapture::format()`, or returned from `start()`) before implementing real capture.
- **The Rust↔TypeScript wire contract (event names, command names, field names) has no test that would catch a typo on either side** — both test suites mock across that boundary independently against themselves. Given this branch already had two real bugs slip past happy-path-only testing, this is flagged as the most valuable test-infrastructure addition for PR 4 to make, not a PR 3 fix.
- **The CSP's one-time "IPC custom protocol failed" console warning has a real fix, not just an unavoidable fallback.** Adding `connect-src 'self' ipc: http://ipc.localhost` to `tauri.conf.json`'s CSP would eliminate the warning and restore Tauri's faster native IPC transport (currently falling back to `postMessage`). This was left as a non-blocking optimization for a future pass since changing it requires re-running the full live `tauri dev` verification cycle — not because the warning is actually unavoidable, correcting the impression this spec's earlier note may have given.
