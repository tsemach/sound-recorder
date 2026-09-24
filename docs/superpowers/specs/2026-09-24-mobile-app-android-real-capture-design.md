# mobile-app: Real Android Audio Capture (sub-project 2)

Status: approved for planning
Scope: `apps/mobile-app`, Android only. Builds on sub-project 1
(scaffold + fake-capture state machine, merged in PR #16).

## Context

Sub-project 1 built the recording state machine, the `AudioCapture` interface
seam, and the Main screen UI against a `FakeCapture` synthetic source. This
sub-project replaces the fake source with real system-audio capture on
Android, using the `AudioPlaybackCapture` API (`android.media.projection` +
`AudioPlaybackCaptureConfiguration`, available from API 29).

iOS real capture is a separate future sub-project — it needs a Mac (Xcode
can never run on this project's Linux dev machine) and a fundamentally
different mechanism (a ReplayKit broadcast extension; there is no direct
system-audio-capture API on iOS). This sub-project is Android-only by
design, not by oversight.

Key PRD constraints this sub-project must honor:
- §6.3: use React Native with native modules for playback capture; detect
  and explain devices that don't permit internal-audio capture; request
  permissions at the point of need with recovery guidance if denied; show
  the platform-required recording indicator.
- §8: isolate platform-specific capture behind the common `AudioCapture`
  interface — this sub-project is the first real test of that seam.
- §9: never start recording silently; clearly indicate active recording.
- §10: actionable errors, never silently discard state.
- §3/§6.3 non-goals and deferred items that still don't apply here:
  microphone capture (still out of MVP scope), and — newly deferred by this
  spec's own scoping decision below — proper WAV encoding, atomic rename,
  storage validation, crash recovery, and audio-focus/interruption/lock-
  screen handling. These depend on either real file writing (next
  sub-project) or are substantial enough to deserve their own scoped work
  rather than being bolted onto "does real capture work at all."

### Environment constraint discovered during this sub-project's brainstorming

The development machine can install the Android SDK and produce full debug
builds (Kotlin + NDK/CMake compiles cleanly, confirmed via a spike — see
below), but **cannot run an Android emulator**: two independent attempts
(default GPU backend, then `-gpu off`) both crashed with SIGSEGV at the same
point during cold boot, despite `/dev/kvm` being present and the CPU having
VMX. This is a sandbox-level incompatibility with QEMU, not a graphics or
resource issue (confirmed no OOM, 41GB RAM free at the time). Consequently:
**this sub-project's native Android code can be compiled and JVM-unit-tested
here, but not run on a device or emulator.** Real on-device verification is
explicitly deferred to whoever has physical Android hardware, exactly as
sub-project 1 deferred all on-device verification (for a different reason —
no SDK at all, at that time).

Two real pnpm-monorepo build issues were found and fixed during the spike
(both already applied to `apps/mobile-app/package.json`, independent of this
spec's own work): `@react-native/gradle-plugin` and `@react-native/codegen`
are transitive dependencies of `react-native` that pnpm's strict linking
doesn't hoist into a workspace package's own `node_modules` — both needed
adding as explicit devDependencies for Gradle's native-module build/codegen
pipeline to find them. This sub-project's own new TurboModule codegen step
depends on that fix already being in place.

## Decisions made during design

- **Keep minSdk 24; detect and explain unsupported OS versions at runtime**,
  rather than bumping minSdk to 29 and dropping older devices. Matches PRD
  §6.3's "detect and explain" language exactly, at the cost of one runtime
  branch (`isSupported()`) instead of a manifest-level cutoff.
- **This sub-project is capture-only, with naive (non-atomic, headerless)
  file output** — mirrors sound-app's own PR4/PR5 split (real capture landed
  before the real WAV writer). The temp file this sub-project produces is
  raw interleaved PCM16 with no container format and no crash-safety
  guarantee; a following sub-project adds the real writer, atomic rename,
  storage validation, and crash recovery. Audio-focus/interruption/lock-
  screen handling is deferred to that same following sub-project, since
  "preserve a recoverable partial file on interruption" (PRD's actual
  requirement here) only makes sense once the real writer exists.
- **A TurboModule, not a legacy bridge module.** `apps/mobile-app`'s
  `android/gradle.properties` already has `newArchEnabled=true`; a legacy
  `NativeModules`/`DeviceEventEmitter` module would fight the app's existing
  configuration rather than follow it.
- **No existing library covers this.** Checked npm for an existing React
  Native `AudioPlaybackCapture` wrapper; nothing exists for this specific,
  niche system API. A custom native module is necessary, not a choice made
  without checking.
- **The `AudioCapture` interface changes — necessarily, not speculatively.**
  Raw PCM buffers arriving every ~20ms cannot reasonably cross the JS
  bridge; they need to stay native and be written straight to a file, the
  same way sound-app's Rust side never sends raw frames to its frontend
  either. Concretely:
  - `start(sourceId, onFrame: (frame: Int16Array) => void)` becomes
    `start(sourceId, onLevel: (level: number) => void)` — capture
    implementations now report a computed level directly instead of hand-
    ing over raw samples for the hook to process.
  - `stop(): Promise<void>` becomes `stop(): Promise<SavedResult>`,
    returning the real file's `filePath`/`durationMs`/`sizeBytes`. This was
    already flagged as a parked "do this when real capture lands" item in
    sub-project 1's final review — this sub-project closes it.
  - `SavedResult` moves from `state/recordingMachine.ts` to
    `capture/types.ts` (it's a capture-produced value; `recordingMachine.ts`
    imports it from there instead of defining it).
  - `FakeCapture` is updated to match: it computes its own synthetic level
    internally (the same RMS math the hook used to do) and calls `onLevel`
    on the same ~10Hz cadence, instead of handing raw frames outward.
  - The hook's `computeLevel`/`latestFrameRef`/frame-tracking logic is
    deleted — capture implementations now own that computation, which is
    where it belongs (the hook shouldn't need to know how a given capture
    source's raw data is shaped). The hook's action surface
    (`state, elapsedMs, level, sources, error, startRecording, ...`) does
    not change.
  - Every other file's shape is unaffected: the state machine, `MainScreen`,
    and every test that only interacts with `AudioCapture` through its
    public methods keep working — only what crosses the interface changes,
    confirming the seam sub-project 1 designed actually holds.

## Architecture

### Permission and consent flow

Requested only when the user presses Record, never at app launch:

1. Runtime request `android.permission.RECORD_AUDIO` if not already granted.
   Required by the `AudioPlaybackCapture` API even though this isn't
   microphone capture — an API quirk worth a code comment so it doesn't
   look like an accidental mic-permission request later.
2. Launch `MediaProjectionManager.createScreenCaptureIntent()` via
   `startActivityForResult` — Android's system consent dialog (the same one
   used for screen recording/casting). Declining is a recoverable `Error`,
   not a crash.
3. On consent granted (`RESULT_OK` + result `Intent`), start a foreground
   service (`AudioCaptureService`, `foregroundServiceType="mediaProjection"`)
   with a persistent notification — required by Android for any
   `MediaProjection`-based capture from API 29+, and this notification
   satisfies PRD §9's "clearly indicate active recording... through
   required platform indicators" as a side effect of the platform
   requirement itself.

Manifest additions: `RECORD_AUDIO`, `FOREGROUND_SERVICE`,
`FOREGROUND_SERVICE_MEDIA_PROJECTION` (required from API 34) permissions,
and the `AudioCaptureService` declaration with
`android:foregroundServiceType="mediaProjection"`.

### Native module structure (`android/app/src/main/java/com/soundrecorder/mobile/audiocapture/`)

```
AudioCaptureModule.kt  — TurboModule: isSupported(), listSources(),
                         startCapture(sourceId), pauseCapture(),
                         resumeCapture(), stopCapture()
AudioCaptureService.kt — foreground service owning the MediaProjection +
                         AudioRecord read loop; writes raw PCM16 to a temp
                         file; computes and emits a level value periodically
AudioCapturePackage.kt — registers the module with React Native
```

TS spec at `apps/mobile-app/src/specs/NativeAudioCapture.ts` (New
Architecture codegen — a `TurboModule`-extending interface), plus a
`codegenConfig` block added to `apps/mobile-app/package.json` pointing at
the specs directory, matching RN's standard TurboModule authoring
convention.

`listSources()` returns a single real entry —
`[{ id: "system-audio", name: "Device Audio" }]` — since
`AudioPlaybackCapture` has no public API for picking a specific other app
to capture from (it captures by audio *usage* category — media, game,
unknown — across all non-opted-out apps, not by app identity). This is a
real capability difference from the fake source's two-entry list and from
desktop's per-device source list; the UI needs no change since `MainScreen`
already renders whatever `listSources()` returns.

### Capture loop

`AudioRecord` is configured using the device's actual output sample rate
(`AudioManager.getProperty(PROPERTY_OUTPUT_SAMPLE_RATE)`, falling back to
48000Hz if unavailable), stereo, 16-bit PCM, with an
`AudioPlaybackCaptureConfiguration` matching `USAGE_MEDIA`, `USAGE_GAME`,
and `USAGE_UNKNOWN`. A dedicated read thread loops on `AudioRecord.read()`,
and per buffer:
- computes RMS-based `level` (0–1) from the buffer, throttled to ~10Hz
  before emitting — the same cadence sub-project 1 established, now
  computed in Kotlin instead of JS;
- appends the raw PCM16 bytes to a fixed temp file under
  `context.filesDir` (e.g. `recording.pcm.tmp`) unless `paused`, mirroring
  `FakeCapture`'s pause semantics (the `AudioRecord` session and read loop
  stay alive across pause/resume; only file writes are skipped).

`stopCapture()` stops the read loop, closes the file, renames it to a
timestamped final name in `filesDir`, and returns
`{ filePath, durationMs, sizeBytes }`. This rename is a plain
`File.renameTo()` — **not** the PRD's crash-safe atomic-write requirement,
which needs real incremental WAV writing to implement meaningfully and is
deferred to the next sub-project along with the container format itself.

### `isSupported()` and version gating

`isSupported()` returns `Build.VERSION.SDK_INT >= 29`. The TS wrapper
(`AndroidPlaybackCapture implements AudioCapture`) checks this before
exposing any sources; when unsupported, `MainScreen` shows an explanatory
message ("System audio recording requires Android 10 or later") instead of
the source picker and Record button. This also fills a gap sub-project 1's
final review flagged and parked: `MainScreen` previously had no
empty/explanatory state when `sources` was empty.

### TS wrapper (`apps/mobile-app/src/capture/androidPlaybackCapture.ts`)

A thin class implementing `AudioCapture`, translating the native module's
promise-based methods and its level event (via the TurboModule's generated
event-emitter, or `NativeEventEmitter` wrapping it — whichever the codegen
tooling produces for this RN version) into the same shape `FakeCapture`
already provides. The hook (`useRecordingState`) does not need to know or
care which implementation it's holding — this is the seam sub-project 1's
design goal was actually about, now proven with a second, real
implementation.

Platform selection (`Platform.OS === 'android'` → `AndroidPlaybackCapture`,
else → `FakeCapture`) happens where the capture instance is constructed
(`MainScreen`/`App.tsx`'s default), not inside the hook itself — the hook
keeps accepting an injected `AudioCapture` exactly as before.

## Testing strategy

**JS side (fully runnable in this environment):**
- `FakeCapture`'s tests updated for the `onLevel` shape instead of
  `onFrame`.
- `recordingMachine.ts`'s `SavedResult` relocation: existing tests updated
  to import from the new location; behavior unchanged.
- `useRecordingState`'s tests simplified (no more synthetic `Int16Array`
  frames to emit) — mocks call `onLevel(value)` directly.
- New tests for `AndroidPlaybackCapture`'s TS wrapper: mock the TurboModule
  and its event emitter (standard Jest/RN pattern — `jest.mock` on the
  generated native module and `NativeEventEmitter`), verifying it correctly
  translates native promise results and level events into the
  `AudioCapture` shape, and that `isSupported() === false` is surfaced
  correctly.

**Kotlin side (compile-verified here; JVM-unit-testable where the code
doesn't touch Android framework classes; NOT runtime-verified here):**
- Any pure logic extracted from the capture loop (e.g., RMS/level
  computation from a PCM16 buffer) gets a plain JVM unit test via
  `./gradlew testDebugUnitTest` (confirmed working during this sub-project's
  toolchain spike).
- `AudioCaptureModule`/`AudioCaptureService`'s actual integration with
  `MediaProjection`/`AudioRecord`/the foreground service lifecycle cannot be
  meaningfully unit-tested (they're thin wrappers over Android framework
  behavior) and cannot be instrumented-tested here (no working emulator).
  Verified by `./gradlew assembleDebug` compiling successfully; real
  behavior needs manual testing on a physical Android device running
  API 29+ before this sub-project is considered fully validated end-to-end.

## Explicitly out of scope for this sub-project

- iOS real capture — separate future sub-project, needs a Mac.
- Proper WAV encoding, atomic rename, storage validation before/during
  recording, and crash recovery of partial files — next sub-project.
- Audio focus changes, phone-call interruption, app suspension, and lock-
  screen behavior (PRD §6.3) — deferred alongside the real file writer,
  since "preserve a recoverable partial file on interruption" depends on
  having real incremental writes to preserve.
- Recordings list screen, Settings screen, navigation — unchanged from
  sub-project 1's own deferral.
- Runtime/on-device manual verification — cannot be performed in this
  development environment (no working Android emulator); required on a
  physical device before this sub-project is fully validated end-to-end.
