# sound-app: Next.js → Tauri Desktop App (Linux MVP)

Status: approved for planning
Scope: `apps/sound-app` only (desktop). `apps/mobile-app` out of scope.

## Context

`apps/sound-app` is currently a bare Next.js 16 (App Router, React 19, Tailwind v4,
`@workspace/ui`) scaffold with no recording capability. Per `PRD.md`, sound-app must
become a Tauri desktop app with a secure Rust backend doing system-audio capture and
file writing, with the frontend only rendering state and issuing commands.

Key PRD constraints driving this design:

- Never start recording silently; recording is always an explicit user action, clearly
  indicated while active (PRD §9).
- Must not bypass DRM, secure audio paths, or platform restrictions (PRD §1, §9).
- Audio stays local by default — no upload, no cloud sync (PRD §2, §9).
- Write audio incrementally (no full-recording-in-memory); use temp files + atomic
  rename on finalize (PRD §8).
- Isolate platform-specific capture behind a common recording interface (PRD §8).

### Decisions made during design

- **Rust/Cargo/Tauri CLI toolchain**: installed locally (rustup stable 1.98.1 via
  rustup.rs; Tauri v2 CLI via `pnpm dlx @tauri-apps/cli`). Fedora build deps
  (`webkit2gtk4.1-devel`, `librsvg2-devel`, `libappindicator-gtk3-devel`,
  `file-devel`, `wget`) installed via `sudo dnf install`. `gtk3-devel` and
  `openssl-devel` were already present.
- **OS priority**: Linux first. This dev machine runs PipeWire behind a
  Pulse-compatible server (`pactl info` → `Server Name: PulseAudio (on PipeWire
  1.6.8)`), with real monitor sources available
  (`alsa_output.pci-0000_00_1f.3.analog-stereo.monitor`), confirming feasibility.
  Windows (WASAPI loopback) and macOS (ScreenCaptureKit) are future work behind the
  same `AudioCapture` trait — not implemented in this pass.
- **Audio format**: WAV (PCM) first — simplest incremental-write path, no encoder
  dependency, satisfies PRD §6.2's "at least one lossless/high-quality format."
  Opus is future work (PRD §13).
- **Linux capture library**: `libpulse-binding` + `libpulse-simple-binding`
  (in-process PulseAudio/PipeWire client bindings), not `cpal` (its Linux ALSA host
  doesn't cleanly enumerate Pulse/PipeWire monitor sources for loopback) and not
  shelling out to `pw-record`/`parec` (would require a Tauri shell-execution
  capability, which PRD §6.2 says to avoid, and makes pause/resume clunky).
- **Frontend stack**: Next.js is removed entirely, replaced with **Vite + React 19 +
  TypeScript** — the stack Tauri's own official templates use. `@workspace/ui` has no
  Next-specific code (`next-themes` works outside Next despite the name), and the
  monorepo already has non-Next shared configs that `packages/ui` itself uses
  (`@workspace/eslint-config/react-internal`, `@workspace/typescript-config/react-library`),
  so this is a convention sound-app adopts, not a special case.
- **CLAUDE.md**: will be updated (as part of PR 1) to describe sound-app as a
  Tauri + Vite + React app instead of Next.js.

## Architecture

### Directory structure

```
apps/sound-app/
  src/                                 ← Vite/React frontend (was app/)
    main.tsx, App.tsx
    components/, hooks/, lib/
  index.html                           ← Vite entry (new)
  vite.config.ts                       ← new
  src-tauri/                           ← new: Rust backend
    Cargo.toml
    tauri.conf.json
    capabilities/                      ← Tauri v2 permission model, restrictive
    src/
      main.rs
      capture/
        mod.rs        ← `AudioCapture` trait (list/start/pause/resume/stop)
        linux_pulse.rs ← libpulse-binding implementation
      writer/
        wav.rs         ← incremental WAV writer: temp file + atomic rename
      recordings/
        mod.rs         ← list/rename/delete, derived from WAV headers on disk
      commands.rs       ← #[tauri::command] functions
      state.rs          ← Tauri managed state (Arc<Mutex<RecordingState>>)
```

Removed: `app/` (App Router), `next.config.ts`, `next-env.d.ts`, the `next` package
dependency. `next-themes` is kept (not Next-specific).

### Frontend ↔ backend contract

Commands (frontend → backend, request/response via `invoke`):

- `list_sources() -> Vec<AudioSource>`
- `start_recording(source_id, format, save_dir) -> ()`
- `pause_recording()`, `resume_recording()`
- `stop_recording() -> RecordingSummary`
- `cancel_recording()` (frontend confirms with the user first, per PRD §6.1)
- `list_recordings() -> Vec<RecordingMeta>`, `delete_recording(id)`, `rename_recording(id, name)`

Events (backend → frontend, push via `emit`):

- `recording-state-changed` — full state snapshot; the single source of truth the
  frontend renders from (no client-side duplication of backend state)
- `recording-tick` — elapsed time + audio level, throttled ~10Hz

Frontend: one `useRecordingState` hook wraps `listen("recording-state-changed")` plus
the command calls; components purely render the current `RecordingState`.

### State machine (Rust, source of truth)

```rust
enum RecordingState {
    Idle,
    Preparing,
    Recording { started_at, elapsed_ms, source_name },
    Paused { elapsed_ms, source_name },
    Saving,
    Saved { file_path, duration_ms, size_bytes },
    Error { message, recoverable: bool },
}
```

States map directly to PRD §6.1 (idle, preparing, recording, paused, saving, saved,
error). `pause`/`resume` gate whether incoming Pulse frames are written, rather than
tearing down and rebuilding the audio stream — cheaper and avoids a capture gap/click
at the resume boundary.

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

`LinuxPulseCapture` is the only implementation for this pass. This trait is the
"common recording interface" PRD §8 requires — future `WindowsWasapiCapture` /
`MacScreenCaptureKitCapture` implementations plug in without touching the state
machine, writer, or frontend.

### Writer & persistence

`WavWriter` owns a `BufWriter<File>` at a temp path
(`<save_dir>/.tmp-<uuid>.wav`), writes a placeholder RIFF header, appends PCM frames
as they arrive, and on `stop()` seeks back to patch the header's size fields, then
`fs::rename`s to the final timestamped name (atomic on the same filesystem) —
satisfying PRD §8's temp-file/atomic-rename requirement. On backend startup, an
orphaned-temp-file sweep finalizes or deletes any leftover `.tmp-*.wav` from a prior
crash (PRD §8's recovery requirement).

No database: the filesystem is the source of truth. `list_recordings()` scans the
save directory and reads duration/sample-rate/channels directly from each WAV
header — no sidecar metadata to drift out of sync. Filenames encode a timestamp
(`recording-2026-09-20T14-32-05.wav`), so "newest first" is a plain sort. Settings
(default save dir, format, filename template) live in Tauri's `Store` plugin
(JSON-backed key-value).

### Error handling

Every `CaptureError`/`IoError` variant maps to a user-facing message + a
`recoverable` flag, surfaced via the `Error` state (PRD §10: denied permissions,
missing source, insufficient storage, failed writes). Disk-space checks run
pre-flight (reject `start_recording` with an actionable error) and periodically
during recording (transition to `Error{recoverable: true}` while preserving
whatever was already written, never silently discarding it).

### Tauri capabilities

Restrictive by default (PRD §6.2): no shell plugin, no network plugin. Filesystem
access scoped to the app's recordings directory plus whatever folder the user
explicitly picks via the `dialog` plugin (native folder picker) — never arbitrary
filesystem access.

## Testing strategy

No test runner exists in the repo yet. Introduce **Vitest** for frontend logic
(primarily `useRecordingState`'s reaction to state/event sequences) and rely on
`cargo test` for the backend: `WavWriter` round-trip tests (write synthetic PCM,
reopen, verify header + samples), state-machine transition tests (illegal
transitions rejected), orphaned-temp-file sweep tests. Per the 80/20 rule: cover the
main record→pause→resume→stop→save flow and the PRD §10 failure paths (denied
source, disk full, write failure) — not exhaustive device/format matrices.

## PR sequence (Linux MVP, WAV-first)

1. **Next.js → Vite migration** — remove `next`, App Router files,
   `next.config.ts`; add Vite + Vitest; port `layout.tsx`/`page.tsx` to
   `main.tsx`/`App.tsx`; switch `tsconfig.json`/`eslint.config.js` to
   `react-library`/`react-internal` (matching `packages/ui`'s existing pattern);
   self-host fonts (`@fontsource/geist-sans`, `@fontsource/geist-mono`) instead of
   `next/font/google`; `components.json` → `"rsc": false`; update `CLAUDE.md`.
   No Tauri yet — proves the frontend stands alone under the new tooling.
2. **Tauri shell** — `src-tauri/` scaffold, restrictive capabilities (no
   shell/network plugins), `tauri.conf.json` wired to Vite dev/build (`devUrl`
   → Vite dev server, `frontendDist` → Vite's `dist/`), a trivial `ping` command
   proving the IPC bridge, Turbo scripts updated. App opens as a native window.
3. **State machine + mocked capture** — `RecordingState` enum, Tauri managed
   state, all commands/events wired end-to-end, backed by a fake
   timer-driven capture source instead of real audio. Full UI (Record/Pause/
   Resume/Stop, elapsed time, state rendering) becomes testable without audio
   hardware in the loop.
4. **Real Linux capture** — `libpulse-binding` integration,
   `LinuxPulseCapture`, real `list_sources()`; swap the fake capture for real
   frames in `start_recording`.
5. **WAV writer + atomic finalize** — incremental PCM writes to temp file,
   header patch + atomic rename on stop, orphaned-temp-file recovery on
   startup. First PR where Record→Stop produces a real, playable file.
6. **Storage checks + error surfacing** — pre-flight and in-flight
   disk-space checks; `Error` state wired to an actual UI error banner with
   actionable messages (PRD §10).
7. **Recordings list screen** — scan save dir, list (date/duration/format/
   size) newest-first; play/rename/delete-with-confirmation/reveal-in-file-manager.
8. **Settings screen** — save-location picker (native dialog, remembered via
   Tauri `Store`), filename template, source/quality selection persisted.

Each PR is independently buildable/reviewable and leaves `main` in a working state.

## Explicitly out of scope for this pass

- Windows/macOS capture backends (trait supports adding them later).
- Opus (or any non-WAV) format.
- Tray/menu-bar controls, continue-recording-when-minimized.
- Accessibility pass (keyboard/screen reader coverage), localization.
- `apps/mobile-app` (separate PRD scope, separate design pass).
