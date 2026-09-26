# mobile-app: Storage Checks + Native Error Surfacing

Status: approved for planning
Scope: `apps/mobile-app`, Android only. Depends on the WAV writer sub-project
(merged: PR #19).

## Context

The recording pipeline is now real end-to-end (real capture, a real WAV
writer, atomic finalize, crash recovery, a real discard path), but nothing
checks whether there's actually free storage to record into, and — unlike
`sound-app`, which already had native failures surfaced to its UI as of its
own PR 5 (writer) before PR 6 (storage checks) added disk-space logic on top
of that — `apps/mobile-app`'s native capture has **no error-reporting
channel to JS at all** yet. `AudioCaptureEngine`'s read-loop catch block
today only logs (`Log.e`) and stops reading; nothing tells the UI, and
nothing tears down the `MediaProjection`/foreground service/notification,
which is currently invisible only because failures are silently swallowed.

This sub-project builds both pieces together, mirroring how `sound-app`
built its own error-surfacing plumbing before layering storage checks on
top of it:

Key PRD requirements this sub-project must honor:
- §6.3: "Handle... low storage."
- §7 (Main Screen): "Display... storage availability."
- §10 (implicit, shared with `sound-app`'s PR 6 framing): validate available
  storage before and during recording; surface actionable errors; preserve
  a partial recording rather than silently discarding it.

### Carryover context this sub-project must account for

- The WAV-writer sub-project's final review confirmed `AudioCaptureEngine`'s
  existing exception handling already leaves the engine in a state a
  subsequent `stop()` call can finalize cleanly — but only if the UI still
  has a way to *call* `stop()` after a failure. It currently doesn't (see
  below), so a mid-recording failure's only recovery path today is the
  crash-recovery scan on next app launch, not an explicit user Stop.
- `apps/mobile-app/src/state/recordingMachine.ts`'s `stop()`/`cancel()` only
  accept `"Recording"`/`"Paused"` as their source state — **not** `"Error"`.
  This exactly mirrors `sound-app`'s own Rust state machine, where
  `can_stop()`/`can_cancel()` are also `false` for `Error` (a carryover gap
  from `sound-app`'s own PR 4/PR 5 review, never closed there either). This
  sub-project does not change that: once a mid-recording failure transitions
  the UI to `Error`, the temp file is preserved via the existing
  crash-recovery scan (already built, from the WAV-writer sub-project) at
  the *next* app launch — not via an explicit Stop tap. This is a deliberate
  parity choice with `sound-app`, not a new gap this sub-project introduces.

## Decisions made during design

- **`android.os.StatFs` for disk-space queries**, not a third-party library.
  Built into the platform (`availableBytes`, available since API 18, well
  under this app's minSdk 24) — an even better fit for this project's
  established minimal-dependency bar than `sound-app`'s `fs4` crate (which
  at least needed a crate; here nothing new is added at all).
- **Fixed 200MB threshold**, matching `sound-app`'s own PR 6 exactly, as a
  shared constant. No settings screen exists yet for either app; making
  this configurable is out of scope here for the same reason it's out of
  scope on the desktop side.
- **Pre-flight check runs immediately after `startCapture()`'s existing early
  guards** (unsupported OS, already-recording, no-activity) **and before any
  permission request or the screen-capture consent dialog.** Those existing
  guards report more fundamental failures ("this device can't do this at
  all", "a recording is already in progress") that should still win over a
  storage message when both are true; once past them, checking storage
  before prompting the user through a system permission/consent flow (only
  to fail afterward) is the same UX win `sound-app`'s "before `capture.start()`
  is called at all" placement gets on the desktop side. Rejects with no
  state change, same as the existing guards.
- **In-flight check inside `AudioCaptureEngine`'s existing read loop**, not
  a separate thread. Unlike `sound-app` (where capture and disk-writing run
  on separate threads connected by a channel, specifically so disk-adjacent
  work never touches the audio-capture thread), `apps/mobile-app`'s
  `AudioCaptureEngine` already does its `AudioRecord.read()` and file write
  in the same loop, on the same thread — there is no separate writer thread
  to put this check on. Throttled to once every 5 seconds, reusing the same
  throttling shape the loop already uses for level-emission (`lastEmitAt`),
  so this adds one cheap comparison per throttle window, not a syscall per
  ~20ms buffer.
- **A new, general native-to-JS failure channel (`AudioCaptureError` RN
  event), built once and used by both the new in-flight storage check and
  the pre-existing (previously silent) read-loop exception path.** This is
  the sub-project's other half, made necessary by the fact that this app,
  unlike `sound-app` at the point it added storage checks, never built this
  plumbing at all. Reuses the existing `AudioCapture`/`useRecordingState`
  `fail()`/error-banner machinery already built for promise-rejection
  failures (e.g. a failed `startCapture()` call) — no new UI, no new state.
- **The read-loop failure path now also tears down the `MediaProjection`
  and foreground service** (mirroring what `stopCapture`/`discardCapture`
  already do), leaving the temp file in place undeleted. This is a small,
  closely-related fix folded into this sub-project rather than deferred:
  without it, surfacing failures to the user (this sub-project's whole
  point) would make a real, pre-existing resource leak newly *visible* —
  the recording notification and system resources would keep running
  indefinitely after a failure the user now knows about but still can't
  stop, because `Error` doesn't permit `stopCapture`/`discardCapture` calls
  from the UI (see above).

## Architecture

### Native (`apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/`)

```
StorageThreshold.kt (new)  — pure, framework-free: the shared MIN_FREE_BYTES
                             constant + isBelowThreshold(freeBytes: Long): Boolean
AudioCaptureEngine.kt      — read loop gains a throttled StatFs check (via
                             StorageThreshold.isBelowThreshold) alongside the
                             existing level-emission throttle; constructor
                             gains an onError: (String) -> Unit callback,
                             invoked by both the storage check and the
                             existing catch block
AudioCaptureModule.kt      — startCapture() gains a pre-flight StatFs check
                             as its first statement; wires AudioCaptureEngine's
                             new onError callback to (a) tear down
                             mediaProjection/AudioCaptureService the same way
                             stopCapture/discardCapture do (without touching
                             the temp file) and (b) emit the new
                             AudioCaptureError RN event
```

**`StorageThreshold.kt`** (new — isolates the one pure comparison so it's
directly unit-testable, mirroring `WavHeader`/`AudioCaptureRecovery`'s
established pattern of small, framework-free objects):

```kotlin
package com.soundrecorder.mobile.audiocapture

object StorageThreshold {
  const val MIN_FREE_BYTES: Long = 200L * 1024 * 1024 // 200 MB

  fun isBelowThreshold(freeBytes: Long): Boolean = freeBytes < MIN_FREE_BYTES
}
```

The `StatFs` query itself (`StatFs(path).availableBytes`) stays inline at
each call site (pre-flight in `AudioCaptureModule`, in-flight in
`AudioCaptureEngine`) — it's a one-line platform call, not worth wrapping,
and (like `AudioRecord`/`MediaProjection`) isn't constructible in this
project's plain JVM unit tests anyway, so wrapping it wouldn't add
testability.

### Pre-flight (`AudioCaptureModule.startCapture()`)

Inserted right after the existing `SDK_INT < Q`, `pendingStartPromise != null || engine != null`,
and `activity == null` guards — all three keep rejecting exactly as they do
today, unchanged — and before `requestRecordAudioPermission(activity)` is
called: compute `StatFs(reactContext.filesDir.path).availableBytes`, and if
`StorageThreshold.isBelowThreshold(...)`, reject with code
`"INSUFFICIENT_STORAGE"` and an actionable message ("Not enough free
storage to start recording (need at least 200MB free)"), returning before
any permission or projection flow starts.

### In-flight (`AudioCaptureEngine`'s read loop)

A new `lastStorageCheckAt: Long` (initialized to `0`, same shape as the
existing `lastEmitAt`) checked on each loop iteration: if 5000ms have
elapsed since the last check, compute `StatFs(outputFile.parentFile.path).availableBytes`
and check it via `StorageThreshold.isBelowThreshold`. Below threshold: log
via `Log.w`, set `running.set(false)` (stops the loop, same shape as the
existing exception path), and call `onError("Recording stopped: storage is
critically low")` — then fall through to the loop's normal exit (no further
frames are read or written).

### Failure teardown + event (`AudioCaptureModule`)

`AudioCaptureEngine`'s constructor gains an `onError: (String) -> Unit`
parameter, passed from `AudioCaptureModule.startEngineNowThatForegroundIsConfirmed()`
alongside the existing `onLevel` callback. The callback:

1. Tears down the same way `discardCapture()` does, *except* it does not
   call `engine.discardAndDelete()` or touch the temp file at all — it only
   nulls `engine`/`mediaProjection`, calls `AudioCaptureService.stop(reactContext)`.
   (The engine's own read loop already stopped itself before calling this
   callback; there's nothing left to tear down on the engine side.)
2. Emits the new event: `reactContext.getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java).emit("AudioCaptureError", message)` —
   the exact same emission shape `emitLevel()` already uses for
   `AudioCaptureLevel`.

The existing read-loop `catch (e: Exception)` block is updated to also call
`onError(e.message ?: "Recording failed unexpectedly")` after logging, so
both failure sources (the new storage check and any other capture
exception) go through the identical teardown-and-report path.

### JS changes

- `apps/mobile-app/src/capture/types.ts`: `AudioCapture.start`'s signature
  gains a third parameter: `start(sourceId: string, onLevel: (level: number) => void, onError: (message: string) => void): Promise<void>`.
- `apps/mobile-app/src/capture/fakeCapture.ts`: accepts the new parameter,
  never calls it (no real failure mode to simulate).
- `apps/mobile-app/src/specs/NativeAudioCapture.ts`: no signature change
  needed (the native module's `startCapture` already only takes `sourceId`
  and a promise — the new event is delivered via the existing
  `NativeEventEmitter`, not a new native method).
- `apps/mobile-app/src/capture/androidPlaybackCapture.ts`: `start()`
  subscribes to the new `"AudioCaptureError"` event (a second
  `emitter.addListener` call, alongside the existing `AudioCaptureLevel`
  one) and forwards the message to `onError`. Both subscriptions are
  tracked and removed together in `stop()`/`discard()`, and both are
  cleaned up if `startCapture` itself rejects (mirroring the existing
  `AudioCaptureLevel` cleanup-on-rejection behavior).
- `apps/mobile-app/src/hooks/useRecordingState.ts`: `startRecording` passes
  a new `onError` callback to `activeCapture.start()` that calls
  `applyState(fail(stateRef.current, message, true))` — `recoverable: true`,
  matching `canStart`'s existing check, so the user can start a fresh
  recording immediately after seeing the error (exactly how the existing
  promise-rejection failure path already behaves).

## Testing strategy

**JVM-runnable in this environment:**
- `StorageThreshold.isBelowThreshold`: unit tests below, at, and above the
  threshold (mirrors `sound-app`'s own `is_below_threshold` test coverage
  exactly).
- JS: `androidPlaybackCapture.test.ts` gains a case asserting an emitted
  `AudioCaptureError` event reaches the `onError` callback, and that both
  subscriptions are removed after `stop()`/`discard()`. `useRecordingState.test.ts`
  gains a case asserting a mock capture's `onError` invocation transitions
  the hook to the `Error` state with `recoverable: true`. `fakeCapture.test.ts`
  confirms the new parameter is accepted without needing a dedicated
  behavior test (it's never called).

**Not unit-testable in this environment (same constraint as prior
sub-projects):** the `StatFs` queries and the full pre-flight/in-flight
integration in `AudioCaptureModule`/`AudioCaptureEngine` — both depend on
real Android framework classes. Verified manually on-device, per
`sound-app`'s own suggested approach: temporarily lower
`StorageThreshold.MIN_FREE_BYTES` for a test build (rather than trying to
manufacture a real near-full disk) to confirm both the pre-flight rejection
and the in-flight stop-and-error path fire correctly, and that the
resulting orphaned temp file is recovered on the next app launch exactly
like any other crash-recovery case.

## Explicitly out of scope for this sub-project

- Making the 200MB threshold configurable — no settings screen exists yet
  for either app (mirrors `sound-app`'s own PR 6 deferral).
- Any new UI beyond reusing the existing error banner — no storage-availability
  indicator on the Main Screen (PRD §7 mentions this, but it's a separate,
  presentational piece of work from the error-path plumbing this sub-project
  builds; a future sub-project can add it once this data is available).
- Extending `Error`'s guarded-transition limitation (allowing an explicit
  Stop/Cancel from `Error` to finalize a partial recording) — this mirrors
  `sound-app`'s own still-open gap and is out of scope here for the same
  reason it's out of scope there.
- Audio focus/interruption/lock-screen handling, iOS, recordings list/export/
  settings screens — unchanged from earlier sub-projects' own deferral.
- Runtime/on-device manual verification — cannot be performed in this
  development environment (no working Android emulator); required on a
  physical device before this sub-project is fully validated end-to-end.
