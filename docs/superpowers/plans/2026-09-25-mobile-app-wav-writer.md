# mobile-app WAV Writer + Atomic Finalize + Crash Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `apps/mobile-app`'s headerless-PCM Android capture output with a real WAV file, atomically finalized on Stop, recoverable after a crash, and properly discarded (not silently finalized) on Cancel.

**Architecture:** A new `WavHeader` object owns the 44-byte canonical PCM WAV header's byte layout (placeholder write + size-field patch at finalize) as pure, framework-free logic. `AudioCaptureEngine` calls it at `start()`/`stop()`, and gains a `discardAndDelete()` teardown path. A new `AudioCaptureRecovery` object reuses the same header logic to scan for orphaned `*.wav.tmp` files at app startup (from `AudioCaptureModule`'s constructor) and either recovers or deletes them. `AudioCaptureModule` gains a `discardCapture()` bridge method; the JS `AudioCapture` interface gains a matching `discard()` that `useRecordingState.cancelRecording` calls instead of `stop()`.

**Tech Stack:** Kotlin (JUnit 4 JVM unit tests — no instrumented/emulator tests are possible in this environment, confirmed during the prior sub-project), React Native/TypeScript, Jest + `@testing-library/react-native`.

**Spec:** `docs/superpowers/specs/2026-09-25-mobile-app-wav-writer-design.md`

## Global Constraints

- Save location is unchanged: recordings stay in `reactContext.filesDir` (app-private internal storage). No MediaStore, no shared/user-visible storage — that's export/share scope, still deferred.
- The WAV header is hand-rolled (44-byte canonical PCM header: RIFF/WAVE/fmt /data), not a third-party library. Format is fixed: 16-bit PCM, stereo (`CHANNEL_COUNT = 2`, matching `AudioCaptureEngine`'s existing `AudioFormat.CHANNEL_IN_STEREO`), sample rate from the engine's existing format negotiation.
- Filenames change from `recording.pcm.tmp`/`recording-<timestamp>.pcm` to `recording.wav.tmp`/`recording-<timestamp>.wav`. Recovered files use the temp file's own `lastModified()` timestamp, not "now," so they sort correctly relative to when they were actually recorded.
- **No Robolectric is configured in this project, and none is added by this plan.** `AudioCaptureEngine.start()`/`stop()` construct real `android.media.AudioRecord`/`MediaProjection` objects, which throw `RuntimeException: ... not mocked` under a plain JVM unit test — this is exactly why the existing `AudioCaptureEngineTest.kt` only tests the static, framework-free `computeLevel` function today. This plan follows the same constraint: `WavHeader` and `AudioCaptureRecovery` are deliberately framework-free (`java.io.File`/`RandomAccessFile` only) so they're fully JVM-testable; `AudioCaptureEngine`'s own integration of them is verified by compilation (`./gradlew assembleDebug`) and manual on-device testing, not new JVM tests.
- Every piece of Kotlin code in this plan has already been compiled and JVM-unit-tested successfully in this exact project during planning (`./gradlew assembleDebug testDebugUnitTest lintDebug` — all green, 0 lint errors) — the code blocks below are the exact verified versions, not first-draft guesses. Every piece of TS/JS code has been verified via `npx tsc --noEmit` (clean) and `npx jest` (110/110 passing).
- `AudioCapture.discard()` mirrors `stop()`'s existing guard pattern: `AndroidPlaybackCapture` always calls through to the native module; the native `discardCapture()` rejects with `NOT_RECORDING` if no capture is active, exactly like `stopCapture()` already does.
- `FakeCapture.discard()` never touches a real file (it never wrote one to begin with) — it's a plain interval-clear.

---

### Task 1: `AudioCapture.discard()` end-to-end (JS/TS)

**Files:**
- Modify: `apps/mobile-app/src/capture/types.ts`
- Modify: `apps/mobile-app/src/capture/fakeCapture.ts`
- Modify: `apps/mobile-app/src/capture/fakeCapture.test.ts`
- Modify: `apps/mobile-app/src/specs/NativeAudioCapture.ts`
- Modify: `apps/mobile-app/src/capture/androidPlaybackCapture.ts`
- Modify: `apps/mobile-app/src/capture/androidPlaybackCapture.test.ts`
- Modify: `apps/mobile-app/src/hooks/useRecordingState.ts`
- Modify: `apps/mobile-app/src/hooks/useRecordingState.test.ts`
- Modify: `apps/mobile-app/src/components/MainScreen.test.tsx`

**Interfaces:**
- Produces: `AudioCapture.discard(): Promise<void>` (consumed by `useRecordingState.cancelRecording`); `Spec.discardCapture(): Promise<void>` on the native module spec (consumed by `AndroidPlaybackCapture.discard()`).
- Consumes: nothing from later tasks — this task is entirely independent of the Kotlin tasks (2-5); the JS side only calls through to a native method name (`discardCapture`) that Task 5 implements. Jest tests mock the native module, so this task's tests pass without any native code changes.

- [ ] **Step 1: Add `discard()` to the `AudioCapture` interface**

In `apps/mobile-app/src/capture/types.ts`, add one line to the interface:

```ts
export type AudioSource = { id: string; name: string }

export type CaptureResult = { filePath: string; sizeBytes: number }

export interface AudioCapture {
  listSources(): Promise<AudioSource[]>
  start(sourceId: string, onLevel: (level: number) => void): Promise<void>
  pause(): void
  resume(): void
  stop(): Promise<CaptureResult>
  discard(): Promise<void>
}
```

- [ ] **Step 2: Write the failing test for `FakeCapture.discard()`**

Add to the end of `apps/mobile-app/src/capture/fakeCapture.test.ts` (inside the existing `describe` block, after the `"resolves stop()..."` test):

```ts
  it("discard() stops reporting levels and resolves", async () => {
    const capture = new FakeCapture()
    const levels: number[] = []
    await capture.start("fake-system-audio", (level) => levels.push(level))

    jest.advanceTimersByTime(40)
    await capture.discard()
    const countAtDiscard = levels.length

    jest.advanceTimersByTime(100)
    expect(levels.length).toBe(countAtDiscard)
  })
```

- [ ] **Step 3: Run the test to confirm it fails**

Run: `npx jest fakeCapture.test.ts`
Expected: FAIL — `capture.discard is not a function` (TypeScript won't have caught this yet since Jest here runs through Babel, not `tsc`).

- [ ] **Step 4: Implement `FakeCapture.discard()`**

In `apps/mobile-app/src/capture/fakeCapture.ts`, add after the existing `stop()` method:

```ts
  async discard(): Promise<void> {
    if (this.intervalId !== null) {
      clearInterval(this.intervalId)
      this.intervalId = null
    }
  }
```

- [ ] **Step 5: Run the test to confirm it passes**

Run: `npx jest fakeCapture.test.ts`
Expected: PASS (7 tests)

- [ ] **Step 6: Add `discardCapture()` to the native spec**

In `apps/mobile-app/src/specs/NativeAudioCapture.ts`, add one line to the `Spec` interface, right after `stopCapture`:

```ts
  stopCapture(): Promise<{ filePath: string; sizeBytes: number }>
  discardCapture(): Promise<void>
  addListener(eventName: string): void
```

- [ ] **Step 7: Write the failing test for `AndroidPlaybackCapture.discard()`**

In `apps/mobile-app/src/capture/androidPlaybackCapture.test.ts`, add `discardCapture` to `mockNativeModule` (right after `stopCapture`):

```ts
  discardCapture: jest.fn(async () => undefined),
```

Then add a new test, right before the `"cleans up the level subscription if startCapture rejects"` test:

```ts
  it("removes the level subscription and calls discardCapture on discard", async () => {
    const capture = new AndroidPlaybackCapture()
    const levels: number[] = []
    await capture.start("system-audio", (level) => levels.push(level))

    await capture.discard()

    expect(NativeAudioCapture.discardCapture).toHaveBeenCalled()
    DeviceEventEmitter.emit("AudioCaptureLevel", 0.9)
    expect(levels).toEqual([])
  })
```

- [ ] **Step 8: Run the test to confirm it fails**

Run: `npx jest androidPlaybackCapture.test.ts`
Expected: FAIL — `capture.discard is not a function`

- [ ] **Step 9: Implement `AndroidPlaybackCapture.discard()`**

In `apps/mobile-app/src/capture/androidPlaybackCapture.ts`, add after the existing `stop()` method:

```ts
  async discard(): Promise<void> {
    this.subscription?.remove()
    this.subscription = null
    await this.nativeModule.discardCapture()
  }
```

- [ ] **Step 10: Run the test to confirm it passes**

Run: `npx jest androidPlaybackCapture.test.ts`
Expected: PASS (7 tests)

- [ ] **Step 11: Wire `cancelRecording` to call `discard()`, and update its test**

In `apps/mobile-app/src/hooks/useRecordingState.test.ts`, add `discardCapture` to the native-module mock (right after `stopCapture`):

```ts
    discardCapture: jest.fn(async () => undefined),
```

Add a `discard` field to `makeMockCapture`'s returned object, right after `stop`:

```ts
    discard: jest.fn(async () => {
      onLevel = null
    }),
```

Update the existing `"cancelRecording discards and returns to Idle"` test to assert the real method call:

```ts
  it("cancelRecording discards and returns to Idle", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    await act(async () => {
      await result.current.cancelRecording()
    })

    expect(result.current.state).toEqual({ state: "Idle" })
    expect(capture.discard).toHaveBeenCalled()
    expect(capture.stop).not.toHaveBeenCalled()
  })
```

- [ ] **Step 12: Run the test to confirm it fails**

Run: `npx jest useRecordingState.test.ts`
Expected: FAIL — `capture.discard` was not called (still calls `stop()`), plus a `not a function` failure from `makeMockCapture` callers that don't yet have `discard` wired into `cancelRecording`.

- [ ] **Step 13: Implement the `cancelRecording` change**

In `apps/mobile-app/src/hooks/useRecordingState.ts`, in `cancelRecording`, change:

```ts
      applyState(cancel(stateRef.current))
      stopTickLoop()
      await activeCapture.stop()
```

to:

```ts
      applyState(cancel(stateRef.current))
      stopTickLoop()
      await activeCapture.discard()
```

- [ ] **Step 14: Run the test to confirm it passes**

Run: `npx jest useRecordingState.test.ts`
Expected: PASS (all tests in this file)

- [ ] **Step 15: Update `MainScreen.test.tsx`'s mocks and tighten its Cancel test**

Add `discardCapture` to the native-module mock (right after `stopCapture`):

```ts
    discardCapture: jest.fn(async () => undefined),
```

Add `discard` to `makeMockCapture`'s returned object, right after `stop`:

```ts
    discard: jest.fn(async () => {}),
```

Add `discard` to the inline `AudioCapture` literal in the `"shows an error banner when loading sources fails"` test, right after its `stop`:

```ts
      discard: jest.fn(async () => {}),
```

Tighten the existing `"confirms before cancelling and discards on confirmation"` test by adding two assertions after its final `waitFor`:

```ts
    await discardButton?.onPress?.()

    await waitFor(() => expect(screen.getByText("Record")).toBeTruthy())
    expect(capture.discard).toHaveBeenCalled()
    expect(capture.stop).not.toHaveBeenCalled()
  })
```

- [ ] **Step 16: Run the full JS suite and typecheck**

Run: `npx tsc --noEmit && npx jest`
Expected: typecheck clean; all test suites passing (110+ tests — this task adds 2 new tests: one in `fakeCapture.test.ts`, one in `androidPlaybackCapture.test.ts`, plus tightens two existing ones).

- [ ] **Step 17: Commit**

```bash
git add apps/mobile-app/src/capture/types.ts apps/mobile-app/src/capture/fakeCapture.ts apps/mobile-app/src/capture/fakeCapture.test.ts apps/mobile-app/src/specs/NativeAudioCapture.ts apps/mobile-app/src/capture/androidPlaybackCapture.ts apps/mobile-app/src/capture/androidPlaybackCapture.test.ts apps/mobile-app/src/hooks/useRecordingState.ts apps/mobile-app/src/hooks/useRecordingState.test.ts apps/mobile-app/src/components/MainScreen.test.tsx
git commit -m "feat(mobile-app): add discard() to AudioCapture, wire Cancel to it"
```

---

### Task 2: `WavHeader` — WAV header byte layout (Kotlin, pure logic)

**Files:**
- Create: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/WavHeader.kt`
- Create: `apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/WavHeaderTest.kt`

**Interfaces:**
- Produces: `WavHeader.HEADER_SIZE: Int` (= 44), `WavHeader.placeholderBytes(sampleRate: Int): ByteArray`, `WavHeader.patchSizes(file: File, totalSize: Long): Unit`. Consumed by Task 3 (`AudioCaptureRecovery`) and Task 4 (`AudioCaptureEngine`).
- Consumes: nothing from other tasks — pure, standalone, `java.io`/`java.nio` only.

- [ ] **Step 1: Write the failing tests**

Create `apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/WavHeaderTest.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Test

class WavHeaderTest {
  @Test
  fun `placeholderBytes has correct RIFF WAVE fmt data layout`() {
    val bytes = WavHeader.placeholderBytes(48000)

    assertEquals(WavHeader.HEADER_SIZE, bytes.size)
    assertEquals("RIFF", String(bytes, 0, 4, Charsets.US_ASCII))
    assertEquals("WAVE", String(bytes, 8, 4, Charsets.US_ASCII))
    assertEquals("fmt ", String(bytes, 12, 4, Charsets.US_ASCII))
    assertEquals("data", String(bytes, 36, 4, Charsets.US_ASCII))

    val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
    assertEquals(0, buffer.getInt(4)) // RIFF chunk size placeholder
    assertEquals(16, buffer.getInt(16)) // fmt chunk size
    assertEquals(1, buffer.getShort(20).toInt()) // audio format = PCM
    assertEquals(2, buffer.getShort(22).toInt()) // channel count
    assertEquals(48000, buffer.getInt(24)) // sample rate
    assertEquals(48000 * 2 * 2, buffer.getInt(28)) // byte rate
    assertEquals(4, buffer.getShort(32).toInt()) // block align
    assertEquals(16, buffer.getShort(34).toInt()) // bits per sample
    assertEquals(0, buffer.getInt(40)) // data chunk size placeholder
  }

  @Test
  fun `patchSizes writes correct RIFF and data chunk sizes`() {
    val tempFile = File.createTempFile("wavheader-test", ".wav")
    tempFile.deleteOnExit()
    try {
      val header = WavHeader.placeholderBytes(48000)
      val dataBytes = ByteArray(1000) { it.toByte() }
      tempFile.writeBytes(header + dataBytes)

      WavHeader.patchSizes(tempFile, tempFile.length())

      val patched = tempFile.readBytes()
      val buffer = ByteBuffer.wrap(patched).order(ByteOrder.LITTLE_ENDIAN)
      assertEquals((tempFile.length() - 8).toInt(), buffer.getInt(4))
      assertEquals(1000, buffer.getInt(40))
    } finally {
      tempFile.delete()
    }
  }
}
```

- [ ] **Step 2: Run the tests to confirm they fail to compile**

Run: `./gradlew :app:testDebugUnitTest --tests "com.soundrecorder.mobile.audiocapture.WavHeaderTest"`
Expected: FAIL to compile — `WavHeader` is unresolved.

- [ ] **Step 3: Implement `WavHeader`**

Create `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/WavHeader.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.ByteOrder

object WavHeader {
  const val HEADER_SIZE = 44
  private const val BITS_PER_SAMPLE = 16
  private const val CHANNEL_COUNT = 2

  fun placeholderBytes(sampleRate: Int): ByteArray {
    val byteRate = sampleRate * CHANNEL_COUNT * BITS_PER_SAMPLE / 8
    val blockAlign = CHANNEL_COUNT * BITS_PER_SAMPLE / 8
    val buffer = ByteBuffer.allocate(HEADER_SIZE).order(ByteOrder.LITTLE_ENDIAN)
    buffer.put("RIFF".toByteArray(Charsets.US_ASCII))
    buffer.putInt(0) // RIFF chunk size — patched at finalize
    buffer.put("WAVE".toByteArray(Charsets.US_ASCII))
    buffer.put("fmt ".toByteArray(Charsets.US_ASCII))
    buffer.putInt(16) // fmt chunk size (PCM)
    buffer.putShort(1) // audio format = PCM
    buffer.putShort(CHANNEL_COUNT.toShort())
    buffer.putInt(sampleRate)
    buffer.putInt(byteRate)
    buffer.putShort(blockAlign.toShort())
    buffer.putShort(BITS_PER_SAMPLE.toShort())
    buffer.put("data".toByteArray(Charsets.US_ASCII))
    buffer.putInt(0) // data chunk size — patched at finalize
    return buffer.array()
  }

  /** [totalSize] is the full file size in bytes, including the 44-byte header. */
  fun patchSizes(file: File, totalSize: Long) {
    val dataLength = totalSize - HEADER_SIZE
    RandomAccessFile(file, "rw").use { raf ->
      raf.seek(4)
      raf.write(leBytes((totalSize - 8).toInt()))
      raf.seek(40)
      raf.write(leBytes(dataLength.toInt()))
    }
  }

  private fun leBytes(value: Int): ByteArray =
    ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(value).array()
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `./gradlew :app:testDebugUnitTest --tests "com.soundrecorder.mobile.audiocapture.WavHeaderTest"`
Expected: PASS (2 tests)

- [ ] **Step 5: Commit**

```bash
git add apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/WavHeader.kt apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/WavHeaderTest.kt
git commit -m "feat(mobile-app): add WavHeader for placeholder-write + finalize-patch WAV byte layout"
```

---

### Task 3: `AudioCaptureRecovery` — orphaned temp-file recovery (Kotlin, pure logic)

**Files:**
- Create: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureRecovery.kt`
- Create: `apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/AudioCaptureRecoveryTest.kt`

**Interfaces:**
- Consumes: `WavHeader.HEADER_SIZE`, `WavHeader.patchSizes` (Task 2).
- Produces: `AudioCaptureRecovery.recoverOrphans(directory: File): Unit`. Consumed by Task 5 (`AudioCaptureModule`'s constructor).

- [ ] **Step 1: Write the failing tests**

Create `apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/AudioCaptureRecoveryTest.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class AudioCaptureRecoveryTest {
  @get:Rule val tempFolder = TemporaryFolder()

  @Test
  fun `deletes an empty temp file with no audio data`() {
    val tempFile = tempFolder.newFile("recording.wav.tmp")
    tempFile.writeBytes(WavHeader.placeholderBytes(48000))

    AudioCaptureRecovery.recoverOrphans(tempFolder.root)

    assertFalse(tempFile.exists())
  }

  @Test
  fun `patches and renames a temp file that has real audio data`() {
    val tempFile = tempFolder.newFile("recording.wav.tmp")
    val header = WavHeader.placeholderBytes(48000)
    val data = ByteArray(200) { it.toByte() }
    tempFile.writeBytes(header + data)

    AudioCaptureRecovery.recoverOrphans(tempFolder.root)

    assertFalse(tempFile.exists())
    val recovered =
      tempFolder.root.listFiles { f -> f.name.startsWith("recording-") && f.name.endsWith(".wav") }
    assertEquals(1, recovered?.size)
    val buffer = ByteBuffer.wrap(recovered!![0].readBytes()).order(ByteOrder.LITTLE_ENDIAN)
    assertEquals(200, buffer.getInt(40))
  }

  @Test
  fun `ignores files that are not temp wav files`() {
    val otherFile = tempFolder.newFile("recording-123.wav")

    AudioCaptureRecovery.recoverOrphans(tempFolder.root)

    assertTrue(otherFile.exists())
  }
}
```

- [ ] **Step 2: Run the tests to confirm they fail to compile**

Run: `./gradlew :app:testDebugUnitTest --tests "com.soundrecorder.mobile.audiocapture.AudioCaptureRecoveryTest"`
Expected: FAIL to compile — `AudioCaptureRecovery` is unresolved.

- [ ] **Step 3: Implement `AudioCaptureRecovery`**

Create `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureRecovery.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import java.io.File

// Recovers *.wav.tmp files left behind by a crash, OS-initiated kill, or an
// interrupted discard — same shape as sound-app's own orphan-recovery, run
// once at app startup rather than as a live in-process error handler.
object AudioCaptureRecovery {
  private const val TEMP_SUFFIX = ".wav.tmp"

  fun recoverOrphans(directory: File) {
    val tempFiles = directory.listFiles { file -> file.name.endsWith(TEMP_SUFFIX) } ?: return
    for (tempFile in tempFiles) {
      recoverOne(tempFile)
    }
  }

  private fun recoverOne(tempFile: File) {
    val size = tempFile.length()
    if (size <= WavHeader.HEADER_SIZE) {
      tempFile.delete()
      return
    }
    WavHeader.patchSizes(tempFile, size)
    val finalFile = File(tempFile.parentFile, "recording-${tempFile.lastModified()}.wav")
    tempFile.renameTo(finalFile)
  }
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `./gradlew :app:testDebugUnitTest --tests "com.soundrecorder.mobile.audiocapture.AudioCaptureRecoveryTest"`
Expected: PASS (3 tests)

- [ ] **Step 5: Commit**

```bash
git add apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureRecovery.kt apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/AudioCaptureRecoveryTest.kt
git commit -m "feat(mobile-app): add AudioCaptureRecovery for orphaned temp-file recovery"
```

---

### Task 4: `AudioCaptureEngine` — real WAV output + discard path

**Files:**
- Modify: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngine.kt`

**Interfaces:**
- Consumes: `WavHeader.placeholderBytes`, `WavHeader.patchSizes` (Task 2).
- Produces: `AudioCaptureEngine.discardAndDelete(): Unit` (new method). `stop(): Long`'s existing signature is unchanged, but its returned value and on-disk output now include a real, finalized WAV header instead of headerless PCM. Consumed by Task 5 (`AudioCaptureModule`).

No new automated tests in this task: `start()`/`stop()` construct real `android.media.AudioRecord`/`MediaProjection` objects, which throw under a plain JVM unit test (see Global Constraints) — exactly why this class's existing test file only covers the static `computeLevel` function. This task's correctness is verified by `./gradlew assembleDebug` (compiles cleanly, already confirmed during planning) and by manual on-device testing (deferred to you with the physical device, per this sub-project's own scope).

- [ ] **Step 1: Write the header to the output stream on `start()`**

In `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngine.kt`, change:

```kotlin
    audioRecord = record
    outputStream = FileOutputStream(outputFile)
    running.set(true)
    paused.set(false)
    record.startRecording()
```

to:

```kotlin
    audioRecord = record
    val stream = FileOutputStream(outputFile)
    stream.write(WavHeader.placeholderBytes(sampleRate))
    outputStream = stream
    running.set(true)
    paused.set(false)
    record.startRecording()
```

- [ ] **Step 2: Extract shared teardown, patch the header on `stop()`, and add `discardAndDelete()`**

Change:

```kotlin
  fun stop(): Long {
    running.set(false)
    audioRecord?.stop()
    thread?.join(2000)
    thread = null
    audioRecord?.release()
    audioRecord = null
    outputStream?.flush()
    outputStream?.close()
    outputStream = null
    return outputFile.length()
  }
}
```

to:

```kotlin
  fun stop(): Long {
    teardownRecording()
    val totalSize = outputFile.length()
    WavHeader.patchSizes(outputFile, totalSize)
    return totalSize
  }

  fun discardAndDelete() {
    teardownRecording()
    outputFile.delete()
  }

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

- [ ] **Step 3: Compile and run the existing unit tests**

Run: `./gradlew assembleDebug testDebugUnitTest`
Expected: compiles cleanly; `AudioCaptureEngineTest`'s 3 existing `computeLevel` tests still pass (this task doesn't touch `computeLevel`).

- [ ] **Step 4: Commit**

```bash
git add apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngine.kt
git commit -m "feat(mobile-app): write and finalize a real WAV header; add discardAndDelete()"
```

---

### Task 5: `AudioCaptureModule` — `discardCapture`, `.wav` output, startup recovery

**Files:**
- Modify: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureModule.kt`

**Interfaces:**
- Consumes: `AudioCaptureEngine.discardAndDelete()` (Task 4), `AudioCaptureRecovery.recoverOrphans()` (Task 3).
- Produces: `discardCapture(promise: Promise)` `@ReactMethod`, matching Task 1's JS-side `discardCapture()` call. `stopCapture`'s finalized file now has a `.wav` extension.

No new automated tests in this task, for the same reason as Task 4 — this class's constructor and bridge methods depend on `ReactApplicationContext`/`Activity`/`MediaProjectionManager`, none of which are constructible in this JVM-only test environment. Verified by `./gradlew assembleDebug lintDebug` (already confirmed clean during planning) and manual on-device testing.

- [ ] **Step 1: Rename the temp file constant and add the startup recovery call**

Change:

```kotlin
    private const val DEFAULT_SAMPLE_RATE = 48000
    private const val TEMP_FILE_NAME = "recording.pcm.tmp"
  }

  override fun getName(): String = NAME

  init {
    reactContext.addActivityEventListener(this)
  }
```

to:

```kotlin
    private const val DEFAULT_SAMPLE_RATE = 48000
    private const val TEMP_FILE_NAME = "recording.wav.tmp"
  }

  override fun getName(): String = NAME

  init {
    reactContext.addActivityEventListener(this)
    AudioCaptureRecovery.recoverOrphans(reactContext.filesDir)
  }
```

- [ ] **Step 2: Change the finalized file's extension, and add `discardCapture`**

Change:

```kotlin
    val tempFile = File(reactContext.filesDir, TEMP_FILE_NAME)
    val finalFile = File(reactContext.filesDir, "recording-${System.currentTimeMillis()}.pcm")
    if (!tempFile.renameTo(finalFile)) {
      promise.reject("RENAME_FAILED", "Could not finalize the recording file")
      return
    }

    val result = Arguments.createMap()
    result.putString("filePath", finalFile.absolutePath)
    result.putDouble("sizeBytes", sizeBytes.toDouble())
    promise.resolve(result)
  }

  @ReactMethod
  fun addListener(eventName: String) {}
```

to:

```kotlin
    val tempFile = File(reactContext.filesDir, TEMP_FILE_NAME)
    val finalFile = File(reactContext.filesDir, "recording-${System.currentTimeMillis()}.wav")
    if (!tempFile.renameTo(finalFile)) {
      promise.reject("RENAME_FAILED", "Could not finalize the recording file")
      return
    }

    val result = Arguments.createMap()
    result.putString("filePath", finalFile.absolutePath)
    result.putDouble("sizeBytes", sizeBytes.toDouble())
    promise.resolve(result)
  }

  @ReactMethod
  @RequiresApi(Build.VERSION_CODES.Q)
  fun discardCapture(promise: Promise) {
    val captureEngine = engine
    if (captureEngine == null) {
      promise.reject("NOT_RECORDING", "No active capture to discard")
      return
    }
    captureEngine.discardAndDelete()
    engine = null
    mediaProjection?.stop()
    mediaProjection = null
    AudioCaptureService.stop(reactContext)
    promise.resolve(null)
  }

  @ReactMethod
  fun addListener(eventName: String) {}
```

- [ ] **Step 3: Build and lint**

Run: `./gradlew assembleDebug testDebugUnitTest lintDebug`
Expected: BUILD SUCCESSFUL; all existing tests pass; 0 lint errors (this was confirmed during planning — `@RequiresApi(Build.VERSION_CODES.Q)` on `discardCapture` matches the pattern already used by `stopCapture`/`pauseCapture`/`resumeCapture`, so no new lint annotations are needed).

- [ ] **Step 4: Commit**

```bash
git add apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureModule.kt
git commit -m "feat(mobile-app): add discardCapture, .wav output, and startup orphan recovery"
```

---

## Manual on-device verification (deferred to physical hardware, per this sub-project's own scope)

Once all 5 tasks are merged, on the physical device used for the prior sub-project's verification:

1. Record real audio, Stop, pull the resulting `.wav` file (`adb shell run-as com.soundrecorder.mobile cat files/recording-<ts>.wav > recording.wav`) and confirm it plays correctly in a standard media player — no `ffmpeg` header-wrapping needed this time, unlike the previous sub-project's raw-PCM verification.
2. Record, Cancel, confirm no `recording-*.wav` or `recording.wav.tmp` file is left in the app's private storage (`adb shell run-as com.soundrecorder.mobile ls files/`).
3. Record, force-kill the app process mid-recording (`adb shell am force-stop com.soundrecorder.mobile`), relaunch, confirm the in-progress recording was recovered as a playable `.wav` file (via the same `run-as ls`/`cat` + playback check as step 1).
