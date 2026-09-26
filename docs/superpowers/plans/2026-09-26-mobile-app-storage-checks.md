# mobile-app Storage Checks + Native Error Surfacing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a pre-flight and in-flight low-storage check to `apps/mobile-app`'s Android capture, and build the general native-to-JS failure-reporting channel this app never had, reusing the existing `Error`-state/error-banner UI already built for promise-rejection failures.

**Architecture:** `StorageThreshold` (new, pure Kotlin) holds the shared 200MB threshold and comparison. `AudioCaptureModule.startCapture()` checks it once, up front, before any permission/consent flow. `AudioCaptureEngine`'s read loop checks it again, throttled every 5 seconds, alongside its existing level-emission throttle. Both the new in-flight check and the pre-existing (previously silent) read-loop exception path now report failures upward through a new `onError` callback the engine calls, which `AudioCaptureModule` turns into a `AudioCaptureError` RN event — mirroring the existing `AudioCaptureLevel` event exactly. On the JS side, `AudioCapture.start()` gains an optional third `onError` parameter that `useRecordingState` wires straight into the `fail()`/error-banner machinery that already exists.

**Tech Stack:** Kotlin (JUnit 4 JVM unit tests — no instrumented/emulator tests are possible in this environment, confirmed across two prior sub-projects), React Native/TypeScript, Jest + `@testing-library/react-native`.

**Spec:** `docs/superpowers/specs/2026-09-26-mobile-app-storage-checks-design.md`

## Global Constraints

- 200MB fixed threshold (`StorageThreshold.MIN_FREE_BYTES`), matching `sound-app`'s own PR 6 exactly. Not configurable — no settings screen exists yet.
- `android.os.StatFs` is the only storage-query mechanism — no new dependency.
- **`AudioCapture.start`'s new `onError` parameter is optional** (`onError?: (message: string) => void`), not required. This is a deliberate refinement over the design spec's exact wording, made during planning: making it required would force every existing test call site across `fakeCapture.test.ts` and `androidPlaybackCapture.test.ts` (~15 call sites) to pass a third argument for no functional reason, since `FakeCapture` never calls it at all. An optional parameter keeps every pre-existing call site valid untouched (verified: `npx tsc --noEmit` stays clean with zero changes to `fakeCapture.ts` or those test files) while `useRecordingState.ts`'s real, production call site always passes it.
- **`FakeCapture` needs no code changes at all for this plan** — confirmed by compiling against the widened interface. Do not add an unused `onError` parameter to it; that would be unrequested surface area.
- The read loop's self-cleanup (`releaseAudioResources()`) must never call `thread?.join(...)` on itself — a thread cannot observe itself as finished while still executing the call that would join it. `teardownRecording()` (called from a different thread, via `stop()`/`discardAndDelete()`) still joins first, then releases; the read loop's own failure path only releases, since by construction it's already about to exit its `while` loop.
- Same JVM-testability constraint as prior sub-projects: `AudioCaptureEngine`/`AudioCaptureModule` construct real `android.media.AudioRecord`/`MediaProjection`/`ReactApplicationContext` objects, which throw under this project's plain JVM unit tests (no Robolectric configured). `StorageThreshold`'s pure comparison is fully JVM-testable; the `StatFs` queries and the full pre-flight/in-flight integration are not — verified by compilation + manual on-device testing only, exactly as `AudioCaptureEngine`'s and `AudioCaptureModule`'s prior changes were.
- Every piece of Kotlin code in this plan has already been compiled and JVM-unit-tested successfully in this exact project during planning (`./gradlew assembleDebug testDebugUnitTest lintDebug` — all green, 0 lint errors) — the code blocks below are the exact verified versions. Every piece of TS/JS code has been verified via `npx tsc --noEmit` (clean) and `npx jest` (115/115 passing).

---

### Task 1: `onError` plumbing end-to-end (JS/TS)

**Files:**
- Modify: `apps/mobile-app/src/capture/types.ts`
- Modify: `apps/mobile-app/src/capture/androidPlaybackCapture.ts`
- Modify: `apps/mobile-app/src/capture/androidPlaybackCapture.test.ts`
- Modify: `apps/mobile-app/src/hooks/useRecordingState.ts`
- Modify: `apps/mobile-app/src/hooks/useRecordingState.test.ts`

**Interfaces:**
- Produces: `AudioCapture.start(sourceId, onLevel, onError?): Promise<void>` — the widened interface. Consumed by Task 4's native `discardCapture`-analogous wiring only indirectly (the native side doesn't know about this JS type; it just emits an `AudioCaptureError` RN event with a `string` payload, which is this task's `AndroidPlaybackCapture` that subscribes to it).
- Consumes: nothing from the Kotlin tasks (2-4) — this task's tests use a mocked native module and a mocked `AudioCapture`, so it's fully independent and can be done before, after, or in parallel with the Kotlin work.
- `FakeCapture`, `fakeCapture.test.ts`, `MainScreen.test.tsx`, `NativeAudioCapture.ts` are all confirmed unaffected by this task (verified during planning: `npx tsc --noEmit` stays clean with none of them touched) — do not edit them.

- [ ] **Step 1: Widen the `AudioCapture` interface**

In `apps/mobile-app/src/capture/types.ts`, change:

```ts
  start(sourceId: string, onLevel: (level: number) => void): Promise<void>
```

to:

```ts
  start(
    sourceId: string,
    onLevel: (level: number) => void,
    onError?: (message: string) => void
  ): Promise<void>
```

- [ ] **Step 2: Confirm the codebase still typechecks with no other changes**

Run: `npx tsc --noEmit`
Expected: clean (0 errors) — this confirms `FakeCapture`'s narrower 2-parameter `start` still satisfies the interface, since the new parameter is optional.

- [ ] **Step 3: Write the failing tests for `AndroidPlaybackCapture`'s error-event forwarding**

In `apps/mobile-app/src/capture/androidPlaybackCapture.test.ts`, add these three tests right after the existing `"cleans up the level subscription if startCapture rejects"` test:

```ts
  it("forwards error events emitted by the native module to the onError callback", async () => {
    const capture = new AndroidPlaybackCapture()
    const errors: string[] = []
    await capture.start(
      "system-audio",
      () => {},
      (message) => errors.push(message)
    )

    DeviceEventEmitter.emit("AudioCaptureError", "storage is critically low")

    expect(errors).toEqual(["storage is critically low"])
    await capture.stop()
  })

  it("stops listening for error events after stop", async () => {
    const capture = new AndroidPlaybackCapture()
    const errors: string[] = []
    await capture.start(
      "system-audio",
      () => {},
      (message) => errors.push(message)
    )
    await capture.stop()

    DeviceEventEmitter.emit("AudioCaptureError", "storage is critically low")

    expect(errors).toEqual([])
  })

  it("does not throw when an error event arrives and no onError callback was given", async () => {
    const capture = new AndroidPlaybackCapture()
    await capture.start("system-audio", () => {})

    expect(() =>
      DeviceEventEmitter.emit("AudioCaptureError", "storage is critically low")
    ).not.toThrow()

    await capture.stop()
  })
```

- [ ] **Step 4: Run the tests to confirm they fail**

Run: `npx jest androidPlaybackCapture.test.ts`
Expected: FAIL — `capture.start` only accepts 2 arguments at the type level in the test file's own usage isn't an issue (JS doesn't enforce arity at runtime), but no `"AudioCaptureError"` listener exists yet, so the first two new tests fail with `errors` staying `[]`.

- [ ] **Step 5: Implement the error-event subscription in `AndroidPlaybackCapture`**

In `apps/mobile-app/src/capture/androidPlaybackCapture.ts`, replace the whole file with:

```ts
import { NativeEventEmitter } from "react-native"

import { getAudioCaptureNativeModule } from "../specs/NativeAudioCapture"
import type { Spec } from "../specs/NativeAudioCapture"
import type { AudioCapture, AudioSource, CaptureResult } from "./types"

const LEVEL_EVENT = "AudioCaptureLevel"
const ERROR_EVENT = "AudioCaptureError"

type AudioCaptureEvents = {
  [LEVEL_EVENT]: [number]
  [ERROR_EVENT]: [string]
}

export class AndroidPlaybackCapture implements AudioCapture {
  private nativeModule: Spec
  private emitter: NativeEventEmitter<AudioCaptureEvents>
  private subscriptions: { remove: () => void }[] = []

  constructor() {
    const nativeModule = getAudioCaptureNativeModule()
    if (nativeModule == null) {
      throw new Error(
        "AudioCapture native module is not available on this platform"
      )
    }
    this.nativeModule = nativeModule
    this.emitter = new NativeEventEmitter<AudioCaptureEvents>(
      this.nativeModule as unknown as ConstructorParameters<
        typeof NativeEventEmitter
      >[0]
    )
  }

  async listSources(): Promise<AudioSource[]> {
    const supported = await this.nativeModule.isSupported()
    if (!supported) return []
    return this.nativeModule.listSources()
  }

  async start(
    sourceId: string,
    onLevel: (level: number) => void,
    onError?: (message: string) => void
  ): Promise<void> {
    this.removeSubscriptions()
    this.subscriptions.push(
      this.emitter.addListener(LEVEL_EVENT, (level: number) => {
        onLevel(level)
      })
    )
    this.subscriptions.push(
      this.emitter.addListener(ERROR_EVENT, (message: string) => {
        onError?.(message)
      })
    )
    try {
      await this.nativeModule.startCapture(sourceId)
    } catch (err) {
      this.removeSubscriptions()
      throw err
    }
  }

  pause(): void {
    this.nativeModule.pauseCapture()
  }

  resume(): void {
    this.nativeModule.resumeCapture()
  }

  async stop(): Promise<CaptureResult> {
    this.removeSubscriptions()
    return this.nativeModule.stopCapture()
  }

  async discard(): Promise<void> {
    this.removeSubscriptions()
    await this.nativeModule.discardCapture()
  }

  private removeSubscriptions(): void {
    for (const subscription of this.subscriptions) {
      subscription.remove()
    }
    this.subscriptions = []
  }
}
```

- [ ] **Step 6: Run the tests to confirm they pass**

Run: `npx jest androidPlaybackCapture.test.ts`
Expected: PASS (10 tests — the existing 7 plus the 3 new ones)

- [ ] **Step 7: Write the failing tests for `useRecordingState`'s mid-recording error handling**

In `apps/mobile-app/src/hooks/useRecordingState.test.ts`, replace the `makeMockCapture` function with:

```ts
function makeMockCapture(sources: AudioSource[]): AudioCapture & {
  emitLevel: (level: number) => void
  emitError: (message: string) => void
} {
  let onLevel: ((level: number) => void) | null = null
  let onError: ((message: string) => void) | null = null
  return {
    listSources: jest.fn(async () => sources),
    start: jest.fn(
      async (
        _sourceId: string,
        cb: (level: number) => void,
        errCb?: (message: string) => void
      ) => {
        onLevel = cb
        onError = errCb ?? null
      }
    ),
    pause: jest.fn(),
    resume: jest.fn(),
    stop: jest.fn(
      async (): Promise<CaptureResult> => {
        onLevel = null
        onError = null
        return { filePath: "mock/recording.wav", sizeBytes: 1024 }
      }
    ),
    discard: jest.fn(async () => {
      onLevel = null
      onError = null
    }),
    emitLevel(level: number) {
      onLevel?.(level)
    },
    emitError(message: string) {
      onError?.(message)
    },
  }
}
```

Then add these two tests right before the `"falls back to FakeCapture on a non-Android platform..."` test:

```ts
  it("transitions to a recoverable Error state when the capture reports a mid-recording error", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    act(() => {
      capture.emitError("storage is critically low")
    })

    expect(result.current.state).toEqual({
      state: "Error",
      message: "storage is critically low",
      recoverable: true,
    })
    expect(result.current.error).toBe("storage is critically low")
  })

  it("stops the tick loop and zeroes the level meter when a mid-recording error arrives", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    act(() => {
      capture.emitLevel(0.8)
    })
    expect(result.current.level).toBe(0.8)
    const timerCountWhileRecording = jest.getTimerCount()

    act(() => {
      capture.emitError("storage is critically low")
    })

    expect(result.current.level).toBe(0)
    // The tick interval is cleared (not just left running uselessly) —
    // compared relatively, since the test environment may hold other
    // unrelated timers of its own.
    expect(jest.getTimerCount()).toBeLessThan(timerCountWhileRecording)
  })
```

- [ ] **Step 8: Run the tests to confirm they fail**

Run: `npx jest useRecordingState.test.ts`
Expected: FAIL — `startRecording` doesn't pass a third callback yet, so `capture.emitError(...)` is a no-op and the state never reaches `Error`.

- [ ] **Step 9: Wire `startRecording`'s `onError` callback**

In `apps/mobile-app/src/hooks/useRecordingState.ts`, change:

```ts
        applyState(prepare(stateRef.current))
        await activeCapture.start(sourceId, (nextLevel) => {
          setLevel(nextLevel)
        })
        startedAtRef.current = Date.now()
```

to:

```ts
        applyState(prepare(stateRef.current))
        await activeCapture.start(
          sourceId,
          (nextLevel) => {
            setLevel(nextLevel)
          },
          (message) => {
            stopTickLoop()
            setLevel(0)
            handleFailure(new Error(message))
          }
        )
        startedAtRef.current = Date.now()
```

(`stopTickLoop` and `handleFailure` are already in `startRecording`'s dependency array — no other changes needed.)

- [ ] **Step 10: Run the tests to confirm they pass**

Run: `npx jest useRecordingState.test.ts`
Expected: PASS (all tests in this file, including the 2 new ones)

- [ ] **Step 11: Run the full JS suite and typecheck**

Run: `npx tsc --noEmit && npx jest`
Expected: typecheck clean; all test suites passing (115 tests total).

- [ ] **Step 12: Commit**

```bash
git add apps/mobile-app/src/capture/types.ts apps/mobile-app/src/capture/androidPlaybackCapture.ts apps/mobile-app/src/capture/androidPlaybackCapture.test.ts apps/mobile-app/src/hooks/useRecordingState.ts apps/mobile-app/src/hooks/useRecordingState.test.ts
git commit -m "feat(mobile-app): add onError to AudioCapture, wire mid-recording failures to the Error state"
```

---

### Task 2: `StorageThreshold` — shared threshold + comparison (Kotlin, pure logic)

**Files:**
- Create: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/StorageThreshold.kt`
- Create: `apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/StorageThresholdTest.kt`

**Interfaces:**
- Produces: `StorageThreshold.MIN_FREE_BYTES: Long`, `StorageThreshold.isBelowThreshold(freeBytes: Long): Boolean`. Consumed by Task 3 (`AudioCaptureEngine`) and Task 4 (`AudioCaptureModule`).
- Consumes: nothing — pure, standalone.

- [ ] **Step 1: Write the failing tests**

Create `apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/StorageThresholdTest.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StorageThresholdTest {
  @Test
  fun `below threshold returns true when free bytes is less than the minimum`() {
    assertTrue(StorageThreshold.isBelowThreshold(StorageThreshold.MIN_FREE_BYTES - 1))
  }

  @Test
  fun `below threshold returns false when free bytes equals the minimum`() {
    assertFalse(StorageThreshold.isBelowThreshold(StorageThreshold.MIN_FREE_BYTES))
  }

  @Test
  fun `below threshold returns false when free bytes is greater than the minimum`() {
    assertFalse(StorageThreshold.isBelowThreshold(StorageThreshold.MIN_FREE_BYTES + 1))
  }
}
```

- [ ] **Step 2: Run the tests to confirm they fail to compile**

Run: `./gradlew :app:testDebugUnitTest --tests "com.soundrecorder.mobile.audiocapture.StorageThresholdTest"`
Expected: FAIL to compile — `StorageThreshold` is unresolved.

- [ ] **Step 3: Implement `StorageThreshold`**

Create `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/StorageThreshold.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

object StorageThreshold {
  const val MIN_FREE_BYTES: Long = 200L * 1024 * 1024 // 200 MB

  fun isBelowThreshold(freeBytes: Long): Boolean = freeBytes < MIN_FREE_BYTES
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `./gradlew :app:testDebugUnitTest --tests "com.soundrecorder.mobile.audiocapture.StorageThresholdTest"`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/StorageThreshold.kt apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/StorageThresholdTest.kt
git commit -m "feat(mobile-app): add StorageThreshold shared low-storage comparison"
```

---

### Task 3: `AudioCaptureEngine` — in-flight storage check + `onError`

**Files:**
- Modify: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngine.kt`

**Interfaces:**
- Consumes: `StorageThreshold.isBelowThreshold` (Task 2).
- Produces: constructor gains a 4th parameter, `onError: (String) -> Unit`, invoked by the read loop on either a caught exception or a detected low-storage condition. Consumed by Task 4 (`AudioCaptureModule`, which must update its `AudioCaptureEngine(...)` construction call to pass this new required parameter — that call site is Task 4's, not this task's, but note it here since it's the direct consumer).

No new automated tests in this task, for the same reason as this class's prior sub-project changes: `start()` constructs a real `android.media.AudioRecord`, which throws under this project's plain JVM unit tests (no Robolectric configured) — exactly why this class's existing tests only cover the static `computeLevel` function. Verified by `./gradlew assembleDebug testDebugUnitTest` (compiles cleanly; existing `computeLevel` tests still pass) and manual on-device testing (deferred to you with the physical device).

- [ ] **Step 1: Add the `onError` constructor parameter and the storage-check interval constant**

In `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngine.kt`, add the import and change the class declaration + companion object:

```kotlin
import android.os.StatFs
```

(add alongside the existing `import android.os.Build`)

```kotlin
class AudioCaptureEngine(
  private val mediaProjection: MediaProjection,
  private val outputFile: File,
  private val onLevel: (Float) -> Unit,
  private val onError: (String) -> Unit,
) {
  companion object {
    private const val LEVEL_EMIT_INTERVAL_MS = 100L
    private const val STORAGE_CHECK_INTERVAL_MS = 5000L
```

(the rest of the companion object's `computeLevel` function is unchanged)

- [ ] **Step 2: Add the throttled in-flight storage check and wire both failure paths to `onError`**

Change the read loop from:

```kotlin
      Thread {
        val buffer = ShortArray(bufferSizeInBytes / 2)
        var lastEmitAt = 0L
        while (running.get()) {
          try {
            val readCount = record.read(buffer, 0, buffer.size)
            if (readCount > 0 && !paused.get()) {
              writeSamples(buffer, readCount)

              val now = System.currentTimeMillis()
              if (now - lastEmitAt >= LEVEL_EMIT_INTERVAL_MS) {
                onLevel(computeLevel(buffer, readCount))
                lastEmitAt = now
              }
            }
          } catch (e: Exception) {
            android.util.Log.e("AudioCaptureEngine", "capture read loop failed", e)
            running.set(false)
          }
        }
      }
```

to:

```kotlin
      Thread {
        val buffer = ShortArray(bufferSizeInBytes / 2)
        var lastEmitAt = 0L
        var lastStorageCheckAt = 0L
        while (running.get()) {
          try {
            val readCount = record.read(buffer, 0, buffer.size)
            if (readCount > 0 && !paused.get()) {
              writeSamples(buffer, readCount)

              val now = System.currentTimeMillis()
              if (now - lastEmitAt >= LEVEL_EMIT_INTERVAL_MS) {
                onLevel(computeLevel(buffer, readCount))
                lastEmitAt = now
              }
              if (now - lastStorageCheckAt >= STORAGE_CHECK_INTERVAL_MS) {
                lastStorageCheckAt = now
                val availableBytes = StatFs(outputFile.path).availableBytes
                if (StorageThreshold.isBelowThreshold(availableBytes)) {
                  android.util.Log.w("AudioCaptureEngine", "stopping: storage below threshold")
                  running.set(false)
                  releaseAudioResources()
                  onError("Recording stopped: storage is critically low")
                }
              }
            }
          } catch (e: Exception) {
            android.util.Log.e("AudioCaptureEngine", "capture read loop failed", e)
            running.set(false)
            releaseAudioResources()
            onError(e.message ?: "Recording failed unexpectedly")
          }
        }
      }
```

- [ ] **Step 3: Extract the resource-release logic so it's safe to call from the read loop's own thread**

Change:

```kotlin
  private fun teardownRecording() {
    running.set(false)
    audioRecord?.stop()
    thread?.join(2000)
    thread = null
    audioRecord?.release()
    audioRecord = null
    outputStream?.flush()
    outputStream?.close()
    outputStream = null
  }
}
```

to:

```kotlin
  private fun teardownRecording() {
    running.set(false)
    thread?.join(2000)
    thread = null
    releaseAudioResources()
  }

  // Called both from teardownRecording() (a different thread joining the read
  // thread first) and directly from inside the read loop's own thread on a
  // fatal failure (self-join would just block pointlessly, since a thread
  // can never observe itself as finished while still running the join call).
  private fun releaseAudioResources() {
    audioRecord?.stop()
    audioRecord?.release()
    audioRecord = null
    outputStream?.flush()
    outputStream?.close()
    outputStream = null
  }
}
```

- [ ] **Step 4: Compile and run the existing unit tests**

Run: `./gradlew assembleDebug testDebugUnitTest`
Expected: compiles cleanly; `AudioCaptureEngineTest`'s 3 existing `computeLevel` tests still pass unchanged.

- [ ] **Step 5: Commit**

```bash
git add apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngine.kt
git commit -m "feat(mobile-app): add in-flight storage check and onError reporting to AudioCaptureEngine"
```

---

### Task 4: `AudioCaptureModule` — pre-flight check + failure teardown/event

**Files:**
- Modify: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureModule.kt`

**Interfaces:**
- Consumes: `StorageThreshold.isBelowThreshold` (Task 2), `AudioCaptureEngine`'s new `onError` constructor parameter (Task 3).
- Produces: the `"INSUFFICIENT_STORAGE"` promise-rejection code (consumed by nothing else in this codebase — it's a terminal JS-facing error code, same as `"UNSUPPORTED"`/`"ALREADY_STARTING"` before it) and the `"AudioCaptureError"` RN event (consumed by Task 1's `AndroidPlaybackCapture`).

No new automated tests in this task, for the same reason as this class's prior sub-project changes — it depends on `ReactApplicationContext`/`Activity`/`MediaProjectionManager`, not constructible under this project's plain JVM unit tests. Verified by `./gradlew assembleDebug testDebugUnitTest lintDebug` (existing tests still pass; 0 lint errors) and manual on-device testing.

- [ ] **Step 1: Add the `StatFs` import and the pre-flight check in `startCapture`**

Add the import alongside the existing `import android.os.Build`:

```kotlin
import android.os.StatFs
```

Change:

```kotlin
    val activity = reactContext.currentActivity
    if (activity == null) {
      promise.reject("NO_ACTIVITY", "No current activity to request capture permission from")
      return
    }
    pendingStartPromise = promise
    requestRecordAudioPermission(activity)
```

to:

```kotlin
    val activity = reactContext.currentActivity
    if (activity == null) {
      promise.reject("NO_ACTIVITY", "No current activity to request capture permission from")
      return
    }
    val availableBytes = StatFs(reactContext.filesDir.path).availableBytes
    if (StorageThreshold.isBelowThreshold(availableBytes)) {
      promise.reject(
        "INSUFFICIENT_STORAGE",
        "Not enough free storage to start recording (need at least 200MB free)",
      )
      return
    }
    pendingStartPromise = promise
    requestRecordAudioPermission(activity)
```

- [ ] **Step 2: Pass the new `onError` callback when constructing the engine**

Change:

```kotlin
      val captureEngine =
        AudioCaptureEngine(
          mediaProjection = projection,
          outputFile = outputFile,
          onLevel = { level -> emitLevel(level) },
        )
```

to:

```kotlin
      val captureEngine =
        AudioCaptureEngine(
          mediaProjection = projection,
          outputFile = outputFile,
          onLevel = { level -> emitLevel(level) },
          onError = { message -> handleCaptureError(message) },
        )
```

- [ ] **Step 3: Add the `handleCaptureError` teardown-and-emit method**

Add this new private method right after the existing `emitLevel` method:

```kotlin
  // Invoked by the engine (from its read-loop thread) when it hits a fatal
  // failure — either the new in-flight storage check or any other capture
  // exception. The engine has already released its own AudioRecord/output
  // stream by this point; this only tears down what the module itself owns
  // (the projection and foreground service), the same way stopCapture/
  // discardCapture do, but leaves the temp file in place undeleted so the
  // existing crash-recovery scan can pick it up on the next app launch —
  // Error doesn't let the UI call stopCapture/discardCapture itself.
  // Only ever invoked via the onError callback passed to AudioCaptureEngine,
  // itself only constructible under the SDK_INT >= Q guard in startCapture().
  @RequiresApi(Build.VERSION_CODES.Q)
  private fun handleCaptureError(message: String) {
    engine = null
    mediaProjection?.stop()
    mediaProjection = null
    AudioCaptureService.stop(reactContext)
    reactContext
      .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
      .emit("AudioCaptureError", message)
  }
```

- [ ] **Step 4: Build, test, and lint**

Run: `./gradlew assembleDebug testDebugUnitTest lintDebug`
Expected: BUILD SUCCESSFUL; all existing tests pass; 0 lint errors. (The `@RequiresApi(Build.VERSION_CODES.Q)` annotation on `handleCaptureError` is required — omitting it fails lint with `NewApi` on the `AudioCaptureService.stop(reactContext)` call, since that method is itself annotated `@RequiresApi(Build.VERSION_CODES.O)` and lint can't trace the guarantee through the lambda passed to `AudioCaptureEngine`'s constructor without it — this was confirmed by hitting exactly that lint failure during planning and fixing it with this annotation.)

- [ ] **Step 5: Commit**

```bash
git add apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureModule.kt
git commit -m "feat(mobile-app): add pre-flight storage check and capture-failure teardown"
```

---

## Manual on-device verification (deferred to physical hardware, per this sub-project's own scope)

Once all 4 tasks are merged, on the physical device used for prior sub-projects' verification, using a temporarily-lowered `StorageThreshold.MIN_FREE_BYTES` (e.g. a value close to the device's actual free space) rather than trying to manufacture a real near-full disk:

1. **Pre-flight rejection:** attempt to start a recording with free space below the (temporarily lowered) threshold; confirm the app shows an actionable error and never shows the screen-capture consent dialog.
2. **In-flight stop-and-error:** start a recording with free space just above the threshold, then consume space on the device (or lower the threshold further mid-recording via a rebuilt debug APK) until the in-flight check fires; confirm the recording stops, an error is shown, and the recording notification/foreground service goes away.
3. **Orphan recovery still works for this failure path:** after step 2, relaunch the app and confirm the interrupted recording is recovered as a playable `.wav`, exactly like the crash-recovery scenario from the prior sub-project.
4. **No regression to the happy path:** with the threshold restored to 200MB (or simply well below actual free space), confirm record → stop → playback, record → cancel → no leftover file, and the promise-rejection error path (e.g. denying the screen-capture consent dialog) all still work exactly as before.
