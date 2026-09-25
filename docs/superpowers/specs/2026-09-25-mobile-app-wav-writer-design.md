# mobile-app: WAV Writer + Atomic Finalize + Crash Recovery

Status: approved for planning
Scope: `apps/mobile-app`, Android only. Depends on the real Android capture
sub-project (merged: PR #17, plus its on-device follow-up fix PR #18).

## Context

The real-capture sub-project made the app record actual system audio, but
`AudioCaptureEngine` writes raw, headerless PCM16 straight to
`recording.pcm.tmp` and `AudioCaptureModule.stopCapture()` renames it to
`recording-<timestamp>.pcm` on Stop — not a real, playable file format, and
with two related gaps:

1. **Cancel doesn't discard.** `useRecordingState.cancelRecording` calls the
   same `activeCapture.stop()` as a real Stop, which finalizes and renames
   the temp file into a real (if headerless) recording — it just doesn't get
   reflected in the app's state. The file is never deleted. This was parked
   during the real-capture sub-project's review as a gap to close later.
2. **No crash/interruption recovery.** If the app or OS kills the process
   mid-recording (crash, OS-initiated kill, force-stop), the temp file is
   left on disk with no header and is never cleaned up or recovered into a
   playable recording.

This sub-project closes both, and makes the output a real WAV file, mirroring
how `sound-app` split its own real-capture work: real capture landed first
(sound-app PR 4), then a following PR (sound-app PR 5) added the real writer,
atomic rename, and crash recovery. `docs/superpowers/specs/2026-09-21-sound-app-wav-writer-design.md`
is the direct precedent this spec follows.

Key PRD constraints this sub-project must honor:
- §6.3: stop safely and preserve a recoverable partial file when the OS
  interrupts capture.
- §8: write audio incrementally (no full-recording-in-memory, already true
  here), use temp files + atomic rename to prevent corrupt visible files,
  recover/clean up temp files after crashes.
- §8: preserve correct sample rate, channels, duration, and container
  metadata (a real WAV header, not headerless PCM).

### Explicitly deferred (own future sub-projects, per user decision)

`sound-app` split storage/low-disk checks into its own PR (PR 6) rather than
bundling it with the writer PR; this spec follows the same split, plus
defers the interruption-handling items that depend on OS lifecycle rather
than file format:

- Low-storage detection/handling (PRD §6.3, §7 "Display... storage
  availability").
- Phone-call/audio-focus interruption, app suspension, and lock-screen
  behavior (PRD §6.3) — these are OS-lifecycle concerns, independently
  testable and reviewable from file-format/crash-recovery work.
- iOS — separate future sub-project, needs a Mac.
- Recordings list screen, export/share actions, Settings — unchanged from
  earlier sub-projects' own deferral.

## Decisions made during design

- **Save location unchanged**: recordings stay in the app's private internal
  storage (`reactContext.filesDir`), exactly where the temp PCM file already
  lives. Making files visible to the user/other apps (MediaStore, a
  user-facing folder) is export/share scope, not this sub-project's — it
  would pull in Android scoped-storage API complexity unrelated to WAV
  writing or crash recovery.
- **Hand-rolled 44-byte canonical PCM WAV header**, not a third-party
  library. The format is fixed and simple (16-bit PCM, known sample
  rate/channel count from the engine's existing format negotiation); unlike
  Rust's `hound` crate (used by `sound-app`), there isn't an equivalently
  well-maintained tiny WAV-writer library for Android worth taking on as a
  dependency. The header is written as a placeholder at `start()`, and its
  two size fields (RIFF chunk size at byte offset 4, `data` sub-chunk size
  at byte offset 40) are patched in place at finalize time — the same
  technique `sound-app`'s orphan-recovery already uses to reconstruct a
  header after the fact for an interrupted file, applied here to the normal
  clean-stop path too.
- **Recovery scan runs from `AudioCaptureModule`'s constructor**, not
  `MainApplication.onCreate()`. The constructor already fires reliably early
  in React Native startup (confirmed during the real-capture sub-project's
  on-device debugging — its constructor log fired consistently), and keeps
  all `AudioCapture`-specific logic contained in one file rather than
  spreading it into `MainApplication`, which currently only wires up the
  package list.
- **Cancel gets a real `discard()` path**, added to this sub-project rather
  than deferred, because it's the direct counterpart to the `Finalize` path
  this sub-project is already building — same shape as `sound-app`'s
  `writer.rs` `Finalize`/`Discard` split on its `WriterMessage` enum. Adding
  it now is a small, closely related change; deferring it would mean
  revisiting the same engine/module code again shortly after for a nearly
  identical shape.
- **`AudioCapture` interface gains `discard(): Promise<void>`.**
  `FakeCapture.discard()` is a trivial interval-clear (it never wrote a real
  file to begin with); `AndroidPlaybackCapture.discard()` calls the new
  native `discardCapture` bridge method.

## Architecture

### Native (`apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/`)

```
AudioCaptureEngine.kt    — start() writes a placeholder 44-byte WAV header
                            before streaming PCM frames; stop() patches the
                            header's size fields and returns the real byte
                            count; new discardAndDelete() tears down without
                            patching and deletes the temp file
AudioCaptureModule.kt    — stopCapture() renames the finalized *.wav.tmp to
                            recording-<timestamp>.wav (same rename call,
                            new extension); new discardCapture() bridge
                            method calls discardAndDelete() and tears down
                            projection/service without renaming; constructor
                            runs the orphan-recovery scan once
WavHeader.kt (new)       — pure functions: writePlaceholderHeader(stream,
                            sampleRate, channelCount), patchHeaderSizes(file,
                            dataLength) — shared by both the normal finalize
                            path and the recovery scan, so the byte-math
                            exists in exactly one place
```

**`WavHeader.kt`** (new file — isolates the byte-format logic so it's
directly unit-testable without touching `AudioRecord`/`MediaProjection`):

```kotlin
package com.soundrecorder.mobile.audiocapture

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
    buffer.put("RIFF".toByteArray())
    buffer.putInt(0) // RIFF chunk size — patched at finalize
    buffer.put("WAVE".toByteArray())
    buffer.put("fmt ".toByteArray())
    buffer.putInt(16) // fmt chunk size (PCM)
    buffer.putShort(1) // audio format = PCM
    buffer.putShort(CHANNEL_COUNT.toShort())
    buffer.putInt(sampleRate)
    buffer.putInt(byteRate)
    buffer.putShort(blockAlign.toShort())
    buffer.putShort(BITS_PER_SAMPLE.toShort())
    buffer.put("data".toByteArray())
    buffer.putInt(0) // data chunk size — patched at finalize
    return buffer.array()
  }

  /** total = full file size in bytes, including the 44-byte header. */
  fun patchSizes(file: java.io.File, total: Long) {
    val dataLength = total - HEADER_SIZE
    RandomAccessFile(file, "rw").use { raf ->
      raf.seek(4)
      raf.write(intToLeBytes((total - 8).toInt()))
      raf.seek(40)
      raf.write(intToLeBytes(dataLength.toInt()))
    }
  }

  private fun intToLeBytes(value: Int): ByteArray =
    ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(value).array()
}
```

`AudioCaptureEngine.start()` writes `WavHeader.placeholderBytes(sampleRate)`
to the `FileOutputStream` before entering the read loop (replacing the
current no-header behavior); PCM frame writes are otherwise unchanged.
`AudioCaptureEngine.stop()` closes the stream, then calls
`WavHeader.patchSizes(outputFile, outputFile.length())`, then returns the
final byte count — the returned size now correctly includes the 44-byte
header. `discardAndDelete()` follows the same shutdown sequence as `stop()`
(stop the read thread, release `AudioRecord`, close the stream) but skips
the header patch and deletes `outputFile` instead of returning its size.

### `AudioCaptureModule.kt` changes

- Temp/final filenames change from `recording.pcm.tmp`/`recording-*.pcm` to
  `recording.wav.tmp`/`recording-*.wav`.
- `stopCapture()`: unchanged shape (calls `engine.stop()`, tears down
  projection/service, renames temp → final), just the new extension.
- New `@ReactMethod fun discardCapture(promise: Promise)`: same guard/
  teardown sequence as `stopCapture()` (projection stop, service stop) but
  calls `engine.discardAndDelete()` instead of `engine.stop()`, and does not
  rename anything (the temp file no longer exists). Resolves the promise
  with `null` on success; rejects with `NOT_RECORDING` if no engine is
  active, matching `stopCapture()`'s existing guard.
- Constructor: after `reactContext.addActivityEventListener(this)`, calls a
  new private `recoverOrphanedRecordings()`:
  1. List `*.wav.tmp` in `reactContext.filesDir`.
  2. For each: if `file.length() <= WavHeader.HEADER_SIZE`, delete it (no
     audio data was ever written — this is what a temp file looks like if
     the crash happened before the engine even started writing frames, or
     if `discardAndDelete()` itself was interrupted mid-delete).
  3. Otherwise, call `WavHeader.patchSizes(file, file.length())`, then
     rename it to `recording-<file's last-modified timestamp>.wav` (using
     the file's own mtime rather than "now," so recovered recordings sort
     correctly relative to when they were actually made).
  This runs synchronously and fast (a directory listing plus, at most, one
  stray temp file) — no async dispatch needed, matching how small and
  bounded `sound-app`'s equivalent startup scan is.

### JS changes

- `apps/mobile-app/src/capture/types.ts`: `AudioCapture` interface gains
  `discard(): Promise<void>`.
- `apps/mobile-app/src/capture/fakeCapture.ts`: `discard()` clears the
  interval (same as `stop()`) and returns — no real file ever existed to
  clean up.
- `apps/mobile-app/src/capture/androidPlaybackCapture.ts`: `discard()` calls
  `this.nativeModule.discardCapture()`.
- `apps/mobile-app/src/specs/NativeAudioCapture.ts`: `Spec` interface gains
  `discardCapture(): Promise<void>`.
- `apps/mobile-app/src/hooks/useRecordingState.ts`: `cancelRecording` calls
  `activeCapture.discard()` instead of `activeCapture.stop()`.

## Testing strategy

**JVM-runnable in this environment (no emulator, matching the established
constraint from the real-capture sub-project):**
- `WavHeader.placeholderBytes()`: assert the returned 44 bytes match the
  expected RIFF/WAVE/fmt /data layout for a given sample rate (check magic
  strings at their fixed offsets, `fmt` fields for the fixed 16-bit-stereo
  format, and that the two size fields are zero).
- `WavHeader.patchSizes()`: write a real temp file (placeholder header + N
  bytes of dummy PCM) to a JVM temp directory, call `patchSizes`, reopen the
  file and assert the two patched fields equal the expected values computed
  from the file's actual size.
- `AudioCaptureModule`'s orphan-recovery logic: extract the scan into a
  small enough shape to unit-test directly (given a directory containing a
  mix of `*.wav.tmp` files of various sizes, assert the right ones get
  deleted vs. patched-and-renamed) — exact test structure decided during
  planning, following this project's practice of validating logic like this
  by actually compiling and running it, not just describing it.
- `AudioCaptureEngine`'s existing JVM unit tests (`AudioCaptureEngineTest.kt`)
  extended to assert `stop()`'s returned size now accounts for the header,
  and that a written temp file, reopened, has a valid patched header.

**JS side (fully runnable in this environment):**
- `androidPlaybackCapture.test.ts`: new case for `discard()` calling
  `discardCapture` on the mock native module.
- `useRecordingState.test.ts`: `cancelRecording` test updated to assert it
  calls `discard()`, not `stop()`.
- `fakeCapture.test.ts`: new case for `discard()`.

**Manual, deferred to physical-device verification (same pattern as the
last two sub-projects):**
- Record real audio, Stop, confirm the resulting `.wav` file plays correctly
  in a standard media player (no `ffmpeg` wrapping needed this time, unlike
  the raw-PCM manual verification used for the previous sub-project).
- Record, Cancel, confirm no file is left in the app's private storage
  (checked via `adb shell run-as <pkg> ls`).
- Record, force-kill the app process mid-recording (`adb shell am kill` or
  equivalent), relaunch, confirm the previously in-progress recording was
  recovered as a playable `.wav` file.

## Explicitly out of scope for this sub-project

- Low-storage detection/handling — own future sub-project (mirrors
  `sound-app` PR 6).
- Audio focus changes, phone-call interruption, app suspension, and
  lock-screen behavior (PRD §6.3) — own future sub-project; independent of
  file-format/crash-recovery concerns.
- iOS real capture — separate future sub-project, needs a Mac.
- Moving finalized recordings to shared/user-visible storage, or any
  export/share action — recordings-list/export scope, still deferred.
- Recordings list screen, Settings screen, navigation — unchanged from
  earlier sub-projects' own deferral.
- Runtime/on-device manual verification — cannot be performed in this
  development environment (no working Android emulator); required on a
  physical device before this sub-project is fully validated end-to-end.
