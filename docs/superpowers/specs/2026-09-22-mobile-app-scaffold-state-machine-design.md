# mobile-app: Scaffold + Recording State Machine with Fake Capture (sub-project 1)

Status: approved for planning
Scope: `apps/mobile-app` only. This directory is currently empty/unscaffolded.

## Context

`apps/mobile-app` is reserved in the PRD for a React Native app but has never
been scaffolded. Building the full mobile app (real system-audio capture,
recordings library, settings, permissions, background/lock-screen behavior)
in one pass mirrors the mistake the desktop app avoided: `sound-app` was
built across 8 incremental PRs, starting with a state machine driven by a
**fake** capture source before tackling real audio capture. This spec
follows the same pattern for mobile, decomposed into its own sub-project so
it can be scoped, built, and reviewed independently.

Real internal-audio capture on mobile is a materially heavier native lift
than the desktop equivalent — Android exposes `AudioPlaybackCapture` only
from API 29, and iOS has no direct system-audio capture API without a
ReplayKit broadcast extension. Building the UI, state machine, and
capture-interface seam first — against a fake source — lets that native
work land later as a drop-in implementation of an already-proven interface,
exactly as `LinuxPulseCapture` later dropped into sound-app's `AudioCapture`
trait.

Key PRD constraints this sub-project must honor:
- §6.1: idle/preparing/recording/paused/saving/saved/error states; show
  Pause/Resume and Stop only when applicable; confirm cancellation.
- §8: isolate platform-specific capture behind a common recording interface.
- §10: actionable errors, never silently discard state.
- §9/§6.3 requirements that depend on *real* audio (permissions, platform
  recording indicator, audio focus/interruption handling, lock-screen
  behavior) do not apply yet — there is no real audio session to manage
  until a later sub-project adds real capture. They are listed explicitly
  under "Out of scope" below rather than left ambiguous.

## Decisions made during design

- **Bare React Native CLI, not Expo.** Custom native modules for
  system-audio capture are coming in a later sub-project; bare RN CLI gives
  full native project control from day one with no Expo prebuild/config-plugin
  layer to work around later.
- **Both iOS and Android from the start.** Since capture is fake in this
  sub-project, there is no native-capability gap between platforms yet — no
  reason to build the shared UI/state machine for only one.
- **Main screen only — no navigation, no Recordings/Settings screens.**
  Mirrors sound-app's own PR3 scope. Recordings list and Settings become
  their own later sub-projects, once real capture and file-writing exist for
  them to operate on.
- **TypeScript is the source of truth for state, not a wire-format mirror.**
  sound-app's TS-side `RecordingState` type uses snake_case fields
  (`elapsed_ms`, `source_name`) only because it mirrors Rust/serde JSON
  crossing the Tauri IPC boundary — Rust's `state.rs` is that app's actual
  source of truth. Mobile has no Rust backend; the TS state machine itself
  is authoritative. Its field names are therefore idiomatic camelCase
  (`elapsedMs`, `sourceName`), matching every other hand-written TS file in
  this repo, not a literal copy of sound-app's snake_case spelling.
- **No new shared `packages/recording-core` package.** Considered and
  rejected: sound-app's TS type is a mirror of a Rust wire format, while
  mobile's TS type is authoritative business logic — they are not the same
  kind of thing, and the camelCase-vs-snake_case difference above means a
  forced shared shape would fight real differences rather than remove
  duplication. This is a premature abstraction today; revisit only if the
  two implementations converge later.
- **`packages/ui` (shadcn/Tailwind, React DOM) is not consumed.** It targets
  the web/DOM rendering model and does not apply to React Native. Mobile-app
  gets its own local `components/`.
- **Jest + `@testing-library/react-native`, not Vitest.** Vitest doesn't
  handle React Native's native-module transforms; Jest is the React Native
  ecosystem standard and what the RN CLI template wires up by default.
- **Formatting stays consistent with the rest of the repo.** The RN CLI
  template's default Prettier config (single quotes, semicolons) is
  replaced with the root `.prettierrc` (no semicolons, double quotes,
  2-space indent) so `pnpm format`/`pnpm lint` behave the same across every
  workspace package.
- **No `build` or `test:rust` script.** Turbo skips a task for any package
  that doesn't define it (confirmed via `packages/ui`, which defines no
  `test`/`build`). Mobile-app has no single meaningful "build" artifact at
  this stage — that becomes Xcode/Gradle's job once real native modules
  exist — and no Rust code.

## Architecture

### Monorepo integration

- `apps/mobile-app` scaffolded via the bare React Native CLI, TypeScript
  template. `pnpm-workspace.yaml` already globs `apps/*`; no workspace
  config change needed once `package.json` exists.
- `metro.config.js` adds `watchFolders` (monorepo root) and adjusts
  `nodeModulesPaths` so Metro resolves hoisted pnpm dependencies correctly —
  the standard fix for React Native + pnpm workspaces.
- `package.json` scripts, matching root Turbo tasks:
  - `dev`: `react-native start` (persistent, matches Turbo's
    `cache: false, persistent: true` dev config)
  - `lint`: `eslint`
  - `format`: `prettier --write` against the root `.prettierrc`
  - `typecheck`: `tsc --noEmit`
  - `test`: `jest`

### Module structure (`apps/mobile-app/src/`)

```
capture/
  types.ts        — AudioCapture interface + AudioSource/CaptureError types
  fakeCapture.ts   — FakeCapture implementing AudioCapture
state/
  recordingMachine.ts — RecordingState union + legal-transition validation
hooks/
  useRecordingState.ts — React hook: owns RecordingState + an injected
                         AudioCapture (defaults to FakeCapture)
components/
  MainScreen.tsx   — the recorder UI (rendered by App.tsx)
lib/
  format.ts        — formatDuration(ms) -> "mm:ss"
```

### State machine

```ts
type AudioSource = { id: string; name: string }

type RecordingState =
  | { state: "Idle" }
  | { state: "Preparing" }
  | { state: "Recording"; sourceName: string; elapsedMs: number }
  | { state: "Paused"; sourceName: string; elapsedMs: number }
  | { state: "Saving" }
  | { state: "Saved"; filePath: string; durationMs: number; sizeBytes: number }
  | { state: "Error"; message: string; recoverable: boolean }
```

**Legal transitions** (everything else throws, caught by the hook and
surfaced as a local error — never silently ignored, per PRD §10):
- `Idle | Saved | Error{recoverable:true}` --(`startRecording`)--> `Preparing` --> `Recording`
- `Recording` ⇄ `Paused` (`pauseRecording` / `resumeRecording`)
- `Recording | Paused` --(`stopRecording`)--> `Saving` --> `Saved`
- `Recording | Paused` --(`cancelRecording`)--> `Idle` (discarded, no `Saved`)

`Saved`'s `filePath`/`sizeBytes` are fake placeholder values in this
sub-project (no real file-writing yet) — field shapes stay as specified so a
later sub-project only needs to supply real values, not change the type.

### Capture interface

```ts
interface AudioCapture {
  listSources(): Promise<AudioSource[]>
  start(sourceId: string, onFrame: (frame: Int16Array) => void): Promise<void>
  pause(): void
  resume(): void
  stop(): Promise<void>
}
```

`FakeCapture` implements this: `listSources()` returns two fake entries
(`fake-system-audio` / `fake-microphone`, matching sound-app's `FakeCapture`
naming). `start()` runs a `setInterval` at ~20ms generating a synthetic
sine-wave sample buffer and invoking `onFrame`. The **hook**, not the
capture implementation, computes `elapsedMs` from frame count and `level`
(RMS-style amplitude) from each buffer, throttled to ~10Hz UI updates — the
same division of responsibility sound-app used so a later native capture
module only has to match `AudioCapture`'s shape, not reimplement this math.

### Hook surface

`useRecordingState()` returns the same shape as sound-app's hook:
`{ state, elapsedMs, level, sources, error, startRecording, pauseRecording,
resumeRecording, stopRecording, cancelRecording }`. Every action wraps
capture/state-machine calls in try/catch, setting a local `error` the UI
renders — illegal transitions and capture failures both surface here,
never fail silently.

### Main screen (`App.tsx` / `MainScreen.tsx`)

Single screen, no navigation:
- Source picker: visible when `Idle | Saved | Error{recoverable:true}`,
  populated from `listSources()`.
- Record button: visible in the same states as the source picker.
- Pause/Resume + Stop + Cancel: visible only when `Recording`/`Paused`.
- Elapsed time formatted `mm:ss` via `lib/format.ts`.
- Level meter: a plain `View` bar tracking `level` (0–1) from the tick
  callback.
- Error banner rendering the hook's `error`.
- Saved confirmation: a visible line ("Saved · {filePath}") shown when
  `state.state === "Saved"`, so the `saved` state is not indistinguishable
  from idle.
- Cancel confirmation via RN's `Alert.alert` (native-confirm equivalent of
  sound-app's Tauri `confirm()` dialog; satisfies PRD's "confirm
  cancellation" requirement with no extra dependency).

## Testing strategy

- Jest unit tests for `recordingMachine`: every illegal transition from
  every state rejected; every legal transition succeeds and updates state
  correctly.
- Jest unit test for `FakeCapture`: produces frames at the expected cadence
  while running; produces none while paused (fake timers, mirroring
  sound-app's `FakeCapture` test coverage).
- Component tests (`@testing-library/react-native`) for `MainScreen`: button
  visibility per state, elapsed time formatting, error banner rendering.
- Manual: `pnpm --filter mobile-app dev` on both an iOS simulator and an
  Android emulator — full click-through (select source → record → pause →
  resume → stop → confirm `Saved`; also cancel mid-recording).

## Explicitly out of scope for this sub-project

- Real native audio capture (Android `AudioPlaybackCapture`, iOS ReplayKit)
  and the native module boundary that will implement `AudioCapture` for
  real — later sub-project.
- Runtime audio/media permissions, audio focus/interruption handling,
  app-suspension/lock-screen behavior, and the platform-required recording
  indicator/notification — all depend on a real audio session that doesn't
  exist until real capture lands.
- Real file writing, storage validation, and crash recovery of partial
  files — depends on real capture producing real audio data.
- Recordings list screen, Settings screen, and any navigation between
  screens — later sub-projects, same order sound-app used.
- A shared `packages/recording-core` type package — rejected for now (see
  Decisions above).
