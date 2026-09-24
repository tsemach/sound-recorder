# mobile-app Android Real Audio Capture Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace `apps/mobile-app`'s fake capture source with real Android system-audio capture via `AudioPlaybackCapture` (API 29+), behind the existing `AudioCapture` interface, with naive (non-atomic, headerless) file output.

**Architecture:** A New Architecture TurboModule (`AudioCaptureModule.kt`) owns the `MediaProjection` consent flow and delegates the actual `AudioRecord` read loop to a small `AudioCaptureEngine` class; a minimal `AudioCaptureService` satisfies Android's foreground-service requirement for `MediaProjection` capture. A thin TS wrapper (`AndroidPlaybackCapture`) implements the existing `AudioCapture` interface by calling the native module and subscribing to a native level event, so the state machine, hook, and UI built in sub-project 1 need no changes beyond the interface's own necessary widening (raw frames can no longer cross the JS bridge).

**Tech Stack:** React Native 0.87 New Architecture (TurboModules), Kotlin, `android.media.projection`/`android.media.AudioRecord`, Jest + `@testing-library/react-native`, JUnit 4 (JVM unit tests only — no instrumented/emulator tests are possible in this environment).

**Spec:** `docs/superpowers/specs/2026-09-24-mobile-app-android-real-capture-design.md`

## Global Constraints

- Keep minSdk 24; detect and explain unsupported OS versions at runtime (`isSupported()` checks `Build.VERSION.SDK_INT >= 29`) rather than bumping minSdk.
- This plan is Android-capture-only, with naive (non-atomic, headerless PCM16) file output — no WAV container, no atomic rename beyond a plain `File.renameTo()`, no storage validation, no crash recovery, no audio-focus/interruption handling. All of that is explicitly deferred to a following sub-project.
- `AudioCapture.start`'s callback changes from `onFrame: (frame: Int16Array) => void` to `onLevel: (level: number) => void`; `stop()` changes from `Promise<void>` to `Promise<CaptureResult>` where `CaptureResult = { filePath: string; sizeBytes: number }` — **not** the full `SavedResult` triple. `durationMs` stays hook-computed from wall clock; `SavedResult` (`{filePath, durationMs, sizeBytes}`) stays exactly where it already is, in `state/recordingMachine.ts`.
- A TurboModule (New Architecture is already enabled: `android/gradle.properties` has `newArchEnabled=true`), not a legacy bridge module.
- Every Kotlin-touching task's verification is `./gradlew assembleDebug` (compiles cleanly) plus, where the code is pure logic with no Android framework dependency, a JVM unit test via `./gradlew :app:testDebugUnitTest`. **No instrumented/emulator testing is possible in this environment** — this has been confirmed twice (the emulator crashes with SIGSEGV at the same boot point regardless of GPU backend). Real on-device verification is deferred to whoever has physical Android hardware.
- Every piece of native-module code and Gradle configuration in this plan has already been compiled successfully in this exact project during planning (a full verification pass, including the JVM unit test) — the code blocks below are the exact verified versions, not first-draft guesses.

---

### Task 1: `AudioCapture` interface refactor (`onLevel`, `CaptureResult`)

**Files:**
- Modify: `apps/mobile-app/src/capture/types.ts`
- Modify: `apps/mobile-app/src/capture/fakeCapture.ts`
- Modify: `apps/mobile-app/src/capture/fakeCapture.test.ts`
- Modify: `apps/mobile-app/src/hooks/useRecordingState.ts`
- Modify: `apps/mobile-app/src/hooks/useRecordingState.test.ts`
- Modify: `apps/mobile-app/src/components/MainScreen.test.tsx`

**Interfaces:**
- Produces: `CaptureResult = { filePath: string; sizeBytes: number }` from `capture/types.ts`; `AudioCapture.start(sourceId, onLevel: (level: number) => void): Promise<void>`; `AudioCapture.stop(): Promise<CaptureResult>`. Consumed by Task 4's `AndroidPlaybackCapture`.
- `state/recordingMachine.ts`'s `SavedResult` type and `finish()` signature are unchanged — do not touch that file in this task.

- [ ] **Step 1: Update `capture/types.ts`**

```ts
export type AudioSource = { id: string; name: string }

export type CaptureResult = { filePath: string; sizeBytes: number }

export interface AudioCapture {
  listSources(): Promise<AudioSource[]>
  start(sourceId: string, onLevel: (level: number) => void): Promise<void>
  pause(): void
  resume(): void
  stop(): Promise<CaptureResult>
}
```

- [ ] **Step 2: Write the failing test for `FakeCapture`'s new shape**

Replace `apps/mobile-app/src/capture/fakeCapture.test.ts` entirely with:

```ts
import { FakeCapture } from "./fakeCapture"

describe("FakeCapture", () => {
  beforeEach(() => {
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.useRealTimers()
  })

  it("lists two fake sources", async () => {
    const capture = new FakeCapture()
    const sources = await capture.listSources()
    expect(sources).toEqual([
      { id: "fake-system-audio", name: "Fake System Audio" },
      { id: "fake-microphone", name: "Fake Microphone" },
    ])
  })

  it("reports levels roughly every 20ms while running", async () => {
    const capture = new FakeCapture()
    const levels: number[] = []
    await capture.start("fake-system-audio", (level) => levels.push(level))

    jest.advanceTimersByTime(100)

    expect(levels.length).toBeGreaterThanOrEqual(4)
    expect(levels[0]).toBeGreaterThan(0)

    await capture.stop()
  })

  it("stops reporting levels while paused", async () => {
    const capture = new FakeCapture()
    const levels: number[] = []
    await capture.start("fake-system-audio", (level) => levels.push(level))

    jest.advanceTimersByTime(40)
    capture.pause()
    const countAtPause = levels.length

    jest.advanceTimersByTime(100)
    expect(levels.length).toBe(countAtPause)

    await capture.stop()
  })

  it("resumes reporting levels after resume", async () => {
    const capture = new FakeCapture()
    const levels: number[] = []
    await capture.start("fake-system-audio", (level) => levels.push(level))

    jest.advanceTimersByTime(40)
    capture.pause()
    const countAtPause = levels.length
    capture.resume()
    jest.advanceTimersByTime(100)

    expect(levels.length).toBeGreaterThan(countAtPause)

    await capture.stop()
  })

  it("stops reporting levels entirely after stop", async () => {
    const capture = new FakeCapture()
    const levels: number[] = []
    await capture.start("fake-system-audio", (level) => levels.push(level))

    jest.advanceTimersByTime(40)
    await capture.stop()
    const countAtStop = levels.length

    jest.advanceTimersByTime(100)
    expect(levels.length).toBe(countAtStop)
  })

  it("throws if start() is called again while already running", async () => {
    const capture = new FakeCapture()
    await capture.start("fake-system-audio", () => {})

    await expect(capture.start("fake-system-audio", () => {})).rejects.toThrow(
      "FakeCapture.start() called while already running"
    )

    await capture.stop()
  })

  it("resolves stop() with a fake file path and zero size", async () => {
    const capture = new FakeCapture()
    await capture.start("fake-system-audio", () => {})

    const result = await capture.stop()

    expect(result.filePath).toMatch(/^fake\/recording-\d+\.wav$/)
    expect(result.sizeBytes).toBe(0)
  })
})
```

- [ ] **Step 3: Run test to verify it fails**

Run: `pnpm --filter mobile-app test fakeCapture.test.ts`
Expected: FAIL — `fakeCapture.ts` still uses the old `onFrame`/`void` shape, so `levels` stays empty and `stop()` resolves to `undefined`.

- [ ] **Step 4: Update `capture/fakeCapture.ts`**

Replace the file entirely with:

```ts
import type { AudioCapture, AudioSource, CaptureResult } from "./types"

const SAMPLE_RATE = 48000
const BUFFER_MS = 20
const FREQUENCY_HZ = 440

function computeLevel(buffer: Int16Array): number {
  if (buffer.length === 0) return 0
  let sumSquares = 0
  for (let i = 0; i < buffer.length; i++) {
    const normalized = buffer[i] / 32768
    sumSquares += normalized * normalized
  }
  return Math.sqrt(sumSquares / buffer.length)
}

export class FakeCapture implements AudioCapture {
  private intervalId: ReturnType<typeof setInterval> | null = null
  private paused = false
  private phase = 0

  async listSources(): Promise<AudioSource[]> {
    return [
      { id: "fake-system-audio", name: "Fake System Audio" },
      { id: "fake-microphone", name: "Fake Microphone" },
    ]
  }

  async start(
    _sourceId: string,
    onLevel: (level: number) => void
  ): Promise<void> {
    if (this.intervalId !== null) {
      throw new Error("FakeCapture.start() called while already running")
    }
    this.paused = false
    this.phase = 0
    const samplesPerBuffer = Math.floor((SAMPLE_RATE * BUFFER_MS) / 1000)

    this.intervalId = setInterval(() => {
      if (this.paused) return
      const buffer = new Int16Array(samplesPerBuffer)
      for (let i = 0; i < samplesPerBuffer; i++) {
        buffer[i] = Math.round(Math.sin(this.phase) * 0.2 * 32767)
        this.phase += (2 * Math.PI * FREQUENCY_HZ) / SAMPLE_RATE
      }
      onLevel(computeLevel(buffer))
    }, BUFFER_MS)
  }

  pause(): void {
    this.paused = true
  }

  resume(): void {
    this.paused = false
  }

  async stop(): Promise<CaptureResult> {
    if (this.intervalId !== null) {
      clearInterval(this.intervalId)
      this.intervalId = null
    }
    return { filePath: `fake/recording-${Date.now()}.wav`, sizeBytes: 0 }
  }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm --filter mobile-app test fakeCapture.test.ts`
Expected: PASS (7 tests)

- [ ] **Step 6: Write the failing test for the hook's new shape**

Replace `apps/mobile-app/src/hooks/useRecordingState.test.ts` entirely with:

```ts
import { act, renderHook, waitFor } from "@testing-library/react-native"

import type { AudioCapture, AudioSource, CaptureResult } from "../capture/types"
import { useRecordingState } from "./useRecordingState"

function makeMockCapture(sources: AudioSource[]): AudioCapture & {
  emitLevel: (level: number) => void
} {
  let onLevel: ((level: number) => void) | null = null
  return {
    listSources: jest.fn(async () => sources),
    start: jest.fn(async (_sourceId: string, cb: (level: number) => void) => {
      onLevel = cb
    }),
    pause: jest.fn(),
    resume: jest.fn(),
    stop: jest.fn(
      async (): Promise<CaptureResult> => {
        onLevel = null
        return { filePath: "mock/recording.wav", sizeBytes: 1024 }
      }
    ),
    emitLevel(level: number) {
      onLevel?.(level)
    },
  }
}

describe("useRecordingState", () => {
  beforeEach(() => {
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.useRealTimers()
  })

  it("loads sources on mount", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))

    await waitFor(() => expect(result.current.sources).toHaveLength(1))
  })

  it("moves Idle -> Preparing -> Recording on startRecording", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    expect(result.current.state).toEqual({
      state: "Recording",
      sourceName: "Source 1",
      elapsedMs: 0,
    })
  })

  it("updates elapsedMs from the tick loop and level directly from onLevel", async () => {
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

    act(() => {
      jest.advanceTimersByTime(100)
    })
    expect(result.current.elapsedMs).toBeGreaterThanOrEqual(100)
  })

  it("rejects pauseRecording while Idle without changing state", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    act(() => {
      result.current.pauseRecording()
    })

    expect(result.current.error).toMatch(/Cannot pause/)
    expect(result.current.state).toEqual({ state: "Idle" })
  })

  it("stopRecording moves Recording to Saved using the capture's result", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    await act(async () => {
      await result.current.stopRecording()
    })

    const state = result.current.state
    expect(state.state).toBe("Saved")
    if (state.state === "Saved") {
      expect(state.filePath).toBe("mock/recording.wav")
      expect(state.sizeBytes).toBe(1024)
    }
  })

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
  })

  it("does not include time spent paused in durationMs when stopping while paused", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    act(() => {
      jest.advanceTimersByTime(2000)
    })

    act(() => {
      result.current.pauseRecording()
    })

    act(() => {
      jest.advanceTimersByTime(60000)
    })

    await act(async () => {
      await result.current.stopRecording()
    })

    const state = result.current.state
    expect(state.state).toBe("Saved")
    if (state.state === "Saved") {
      expect(state.durationMs).toBeLessThan(3000)
    }
  })

  it("stops the capture on unmount while a recording is active", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result, unmount } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    unmount()

    expect(capture.stop).toHaveBeenCalled()
  })

  it("zeroes the level meter when pausing", async () => {
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

    act(() => {
      result.current.pauseRecording()
    })

    expect(result.current.level).toBe(0)
  })

  it("transitions to a recoverable Error state when capture.start rejects", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    capture.start = jest.fn(async () => {
      throw new Error("device busy")
    })
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    expect(result.current.state).toEqual({
      state: "Error",
      message: "device busy",
      recoverable: true,
    })
    expect(result.current.error).toBe("device busy")
  })
})
```

- [ ] **Step 7: Run test to verify it fails**

Run: `pnpm --filter mobile-app test useRecordingState.test.ts`
Expected: FAIL — the hook still expects `onFrame`/computes level from a buffered frame ref, and `stopRecording` still fabricates its own `filePath`.

- [ ] **Step 8: Update `hooks/useRecordingState.ts`**

Replace the file entirely with:

```ts
import { useCallback, useEffect, useRef, useState } from "react"

import type { AudioCapture, AudioSource } from "../capture/types"
import { FakeCapture } from "../capture/fakeCapture"
import { errorMessage } from "../lib/errorMessage"
import {
  IllegalTransitionError,
  RecordingState,
  begin,
  cancel,
  fail,
  finish,
  pause,
  prepare,
  resume,
  stop,
  updateElapsed,
} from "../state/recordingMachine"

const TICK_MS = 100

export function useRecordingState(capture?: AudioCapture) {
  const fallbackRef = useRef<AudioCapture | null>(null)
  if (fallbackRef.current === null) {
    fallbackRef.current = new FakeCapture()
  }
  const activeCapture = capture ?? fallbackRef.current

  const [state, setState] = useState<RecordingState>({ state: "Idle" })
  const [level, setLevel] = useState(0)
  const [sources, setSources] = useState<AudioSource[]>([])
  const [error, setError] = useState<string | null>(null)

  const stateRef = useRef<RecordingState>(state)
  const startedAtRef = useRef(0)
  const pausedAccumRef = useRef(0)
  const pausedAtRef = useRef(0)
  const tickIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const applyState = useCallback((next: RecordingState) => {
    stateRef.current = next
    setState(next)
  }, [])

  const handleFailure = useCallback(
    (err: unknown) => {
      if (err instanceof IllegalTransitionError) {
        setError(errorMessage(err))
        return
      }
      applyState(fail(stateRef.current, errorMessage(err), true))
      setError(errorMessage(err))
    },
    [applyState]
  )

  useEffect(() => {
    let cancelled = false
    activeCapture
      .listSources()
      .then((result) => {
        if (!cancelled) setSources(result)
      })
      .catch((err) => {
        if (!cancelled) setError(errorMessage(err))
      })
    return () => {
      cancelled = true
    }
  }, [activeCapture])

  const stopTickLoop = useCallback(() => {
    if (tickIntervalRef.current !== null) {
      clearInterval(tickIntervalRef.current)
      tickIntervalRef.current = null
    }
  }, [])

  const startTickLoop = useCallback(() => {
    stopTickLoop()
    tickIntervalRef.current = setInterval(() => {
      const elapsedMs =
        Date.now() - startedAtRef.current - pausedAccumRef.current
      applyState(updateElapsed(stateRef.current, elapsedMs))
    }, TICK_MS)
  }, [applyState, stopTickLoop])

  useEffect(() => {
    return () => {
      stopTickLoop()
      void activeCapture.stop()
    }
    // Empty deps: this must run only on actual unmount, not whenever
    // activeCapture/stopTickLoop identity changes. activeCapture is stable
    // across renders when no capture prop is passed (see the lazy fallback
    // ref above).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const startRecording = useCallback(
    async (sourceId: string) => {
      setError(null)
      try {
        const source = sources.find((candidate) => candidate.id === sourceId)
        const sourceName = source?.name ?? sourceId
        applyState(prepare(stateRef.current))
        await activeCapture.start(sourceId, (level) => {
          setLevel(level)
        })
        startedAtRef.current = Date.now()
        pausedAccumRef.current = 0
        applyState(begin(stateRef.current, sourceName))
        startTickLoop()
      } catch (err) {
        stopTickLoop()
        handleFailure(err)
      }
    },
    [
      activeCapture,
      applyState,
      handleFailure,
      sources,
      startTickLoop,
      stopTickLoop,
    ]
  )

  const pauseRecording = useCallback(() => {
    setError(null)
    try {
      applyState(pause(stateRef.current))
      activeCapture.pause()
      pausedAtRef.current = Date.now()
      stopTickLoop()
      setLevel(0)
    } catch (err) {
      handleFailure(err)
    }
  }, [activeCapture, applyState, handleFailure, stopTickLoop])

  const resumeRecording = useCallback(() => {
    setError(null)
    try {
      applyState(resume(stateRef.current))
      activeCapture.resume()
      pausedAccumRef.current += Date.now() - pausedAtRef.current
      startTickLoop()
    } catch (err) {
      handleFailure(err)
    }
  }, [activeCapture, applyState, handleFailure, startTickLoop])

  const stopRecording = useCallback(async () => {
    setError(null)
    try {
      if (stateRef.current.state === "Paused") {
        pausedAccumRef.current += Date.now() - pausedAtRef.current
      }
      applyState(stop(stateRef.current))
      stopTickLoop()
      const { filePath, sizeBytes } = await activeCapture.stop()
      const durationMs =
        Date.now() - startedAtRef.current - pausedAccumRef.current
      applyState(finish(stateRef.current, { filePath, durationMs, sizeBytes }))
    } catch (err) {
      handleFailure(err)
    }
  }, [activeCapture, applyState, handleFailure, stopTickLoop])

  const cancelRecording = useCallback(async () => {
    setError(null)
    try {
      applyState(cancel(stateRef.current))
      stopTickLoop()
      await activeCapture.stop()
    } catch (err) {
      handleFailure(err)
    }
  }, [activeCapture, applyState, handleFailure, stopTickLoop])

  const elapsedMs =
    state.state === "Recording" || state.state === "Paused"
      ? state.elapsedMs
      : 0

  return {
    state,
    elapsedMs,
    level,
    sources,
    error,
    startRecording,
    pauseRecording,
    resumeRecording,
    stopRecording,
    cancelRecording,
  }
}
```

- [ ] **Step 9: Run test to verify it passes**

Run: `pnpm --filter mobile-app test useRecordingState.test.ts`
Expected: PASS (10 tests)

- [ ] **Step 10: Update `MainScreen.test.tsx`'s mock captures to the new `stop()` shape**

`apps/mobile-app/src/components/MainScreen.test.tsx` has two places constructing a mock `AudioCapture` with a `stop` mock that must now resolve to a `CaptureResult` instead of `undefined` (required for TypeScript to accept them against the updated `AudioCapture` interface). Both changes keep the existing "shows a saved confirmation after stopping" test's `/^Saved ·/` regex assertion passing, since it doesn't check the exact path.

In the `makeMockCapture` helper near the top of the file, change:

```ts
function makeMockCapture(sources: AudioSource[]): AudioCapture {
  return {
    listSources: jest.fn(async () => sources),
    start: jest.fn(async () => {}),
    pause: jest.fn(),
    resume: jest.fn(),
    stop: jest.fn(async () => {}),
  }
}
```

to:

```ts
function makeMockCapture(sources: AudioSource[]): AudioCapture {
  return {
    listSources: jest.fn(async () => sources),
    start: jest.fn(async () => {}),
    pause: jest.fn(),
    resume: jest.fn(),
    stop: jest.fn(async () => ({ filePath: "mock/recording.wav", sizeBytes: 1024 })),
  }
}
```

In the "shows an error banner when loading sources fails" test's inline `AudioCapture` object, change:

```ts
    const capture: AudioCapture = {
      listSources: jest.fn(async () => {
        throw new Error("mic unavailable")
      }),
      start: jest.fn(async () => {}),
      pause: jest.fn(),
      resume: jest.fn(),
      stop: jest.fn(async () => {}),
    }
```

to:

```ts
    const capture: AudioCapture = {
      listSources: jest.fn(async () => {
        throw new Error("mic unavailable")
      }),
      start: jest.fn(async () => {}),
      pause: jest.fn(),
      resume: jest.fn(),
      stop: jest.fn(async () => ({ filePath: "mock/recording.wav", sizeBytes: 1024 })),
    }
```

No other change to this file in this task — the `FakeCapture`-based "uses a single stable FakeCapture instance" test needs no edit since it doesn't touch `stop()`'s return value.

- [ ] **Step 11: Run the full mobile-app suite and typecheck**

```bash
pnpm --filter mobile-app typecheck
pnpm --filter mobile-app lint
pnpm --filter mobile-app test
```

Expected: all pass. Test count should be the same as before this task (7 + 10 + existing `MainScreen`/`recordingMachine`/`format` suites) since this task only changed shapes, not coverage.

- [ ] **Step 12: Commit**

```bash
git add apps/mobile-app/src/capture/types.ts apps/mobile-app/src/capture/fakeCapture.ts apps/mobile-app/src/capture/fakeCapture.test.ts apps/mobile-app/src/hooks/useRecordingState.ts apps/mobile-app/src/hooks/useRecordingState.test.ts apps/mobile-app/src/components/MainScreen.test.tsx
git commit -m "refactor(mobile-app): widen AudioCapture to onLevel/CaptureResult for real capture"
```

---

### Task 2: TurboModule scaffold (spec, codegen, stub Kotlin module)

**Files:**
- Create: `apps/mobile-app/src/specs/NativeAudioCapture.ts`
- Modify: `apps/mobile-app/package.json` (add `codegenConfig`)
- Create: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureModule.kt`
- Create: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCapturePackage.kt`
- Modify: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/MainApplication.kt`

**Interfaces:**
- Produces: the codegen'd `NativeAudioCaptureSpec` abstract Java class (generated at build time into `android/app/build/generated/source/codegen/java/com/soundrecorder/mobile/audiocapture/NativeAudioCaptureSpec.java` — do not hand-write or commit this file, it's build output) that `AudioCaptureModule` extends. Produces a default-exported `NativeAudioCapture` TS module from `src/specs/NativeAudioCapture.ts`, consumed by Task 4's `AndroidPlaybackCapture`.
- This task's Kotlin method bodies are stubs (no real capture logic yet) — real logic is Task 3. This task's deliverable is proving the codegen → Kotlin → registration pipeline compiles and the module is wired into the app, independent of whether it does anything real yet.

- [ ] **Step 1: Write the TurboModule TS spec**

```ts
import type { TurboModule } from "react-native"
import { TurboModuleRegistry } from "react-native"

export interface Spec extends TurboModule {
  isSupported(): Promise<boolean>
  listSources(): Promise<Array<{ id: string; name: string }>>
  startCapture(sourceId: string): Promise<void>
  pauseCapture(): void
  resumeCapture(): void
  stopCapture(): Promise<{ filePath: string; sizeBytes: number }>
  addListener(eventName: string): void
  removeListeners(count: number): void
}

export default TurboModuleRegistry.getEnforcing<Spec>("AudioCapture")
```

Save as `apps/mobile-app/src/specs/NativeAudioCapture.ts`.

- [ ] **Step 2: Add `codegenConfig` to `package.json`**

Add this top-level key to `apps/mobile-app/package.json` (alongside `"scripts"`, `"dependencies"`, etc.):

```json
"codegenConfig": {
  "name": "AudioCaptureSpec",
  "type": "modules",
  "jsSrcsDir": "src/specs",
  "android": {
    "javaPackageName": "com.soundrecorder.mobile.audiocapture"
  }
}
```

- [ ] **Step 3: Verify the codegen schema and Java spec generate correctly**

```bash
cd apps/mobile-app/android
./gradlew :app:generateCodegenSchemaFromJavaScript --console=plain
./gradlew :app:generateCodegenArtifactsFromSchema --console=plain
```

Expected: both `BUILD SUCCESSFUL`. Confirm the generated file exists:

```bash
cat apps/mobile-app/android/app/build/generated/source/codegen/java/com/soundrecorder/mobile/audiocapture/NativeAudioCaptureSpec.java
```

Expected: an abstract class `NativeAudioCaptureSpec extends ReactContextBaseJavaModule implements TurboModule` with abstract methods `isSupported(Promise)`, `listSources(Promise)`, `startCapture(String, Promise)`, `pauseCapture()`, `resumeCapture()`, `stopCapture(Promise)`, `addListener(String)`, `removeListeners(double)`.

- [ ] **Step 4: Write the stub Kotlin module**

Create `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureModule.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import android.os.Build
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext

class AudioCaptureModule(reactContext: ReactApplicationContext) :
  NativeAudioCaptureSpec(reactContext) {

  companion object {
    const val NAME = "AudioCapture"
  }

  override fun isSupported(promise: Promise) {
    promise.resolve(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
  }

  override fun listSources(promise: Promise) {
    val source = Arguments.createMap()
    source.putString("id", "system-audio")
    source.putString("name", "Device Audio")
    val sources = Arguments.createArray()
    sources.pushMap(source)
    promise.resolve(sources)
  }

  override fun startCapture(sourceId: String, promise: Promise) {
    promise.resolve(null)
  }

  override fun pauseCapture() {}

  override fun resumeCapture() {}

  override fun stopCapture(promise: Promise) {
    val result = Arguments.createMap()
    result.putString("filePath", "")
    result.putDouble("sizeBytes", 0.0)
    promise.resolve(result)
  }

  override fun addListener(eventName: String) {}

  override fun removeListeners(count: Double) {}
}
```

Note: `NAME` must be a `companion object { const val NAME = ... }` on `AudioCaptureModule` itself — Kotlin does not cleanly resolve the inherited Java static `NAME` field from `NativeAudioCaptureSpec` through the subclass reference (confirmed by compiler error during planning: `Unresolved reference 'NAME'`).

- [ ] **Step 5: Write the package registration**

Create `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCapturePackage.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import com.facebook.react.BaseReactPackage
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.module.model.ReactModuleInfo
import com.facebook.react.module.model.ReactModuleInfoProvider

class AudioCapturePackage : BaseReactPackage() {
  override fun getModule(
    name: String,
    reactContext: ReactApplicationContext,
  ): NativeModule? {
    return if (name == AudioCaptureModule.NAME) {
      AudioCaptureModule(reactContext)
    } else {
      null
    }
  }

  override fun getReactModuleInfoProvider(): ReactModuleInfoProvider {
    return ReactModuleInfoProvider {
      mapOf(
        AudioCaptureModule.NAME to
          ReactModuleInfo(
            AudioCaptureModule.NAME,
            AudioCaptureModule.NAME,
            false,
            false,
            false,
            true,
          )
      )
    }
  }
}
```

- [ ] **Step 6: Register the package in `MainApplication.kt`**

In `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/MainApplication.kt`, add the import and registration:

```kotlin
import com.facebook.react.defaults.DefaultReactHost.getDefaultReactHost
import com.soundrecorder.mobile.audiocapture.AudioCapturePackage

class MainApplication : Application(), ReactApplication {

  override val reactHost: ReactHost by lazy {
    getDefaultReactHost(
      context = applicationContext,
      packageList =
        PackageList(this).packages.apply {
          add(AudioCapturePackage())
        },
    )
  }
```

(Replace the existing commented-out placeholder line `// add(MyReactNativePackage())` with `add(AudioCapturePackage())` and add the import above `class MainApplication`.)

- [ ] **Step 7: Verify the full build**

```bash
cd apps/mobile-app/android
./gradlew assembleDebug --console=plain
```

Expected: `BUILD SUCCESSFUL`. This compiles the Kotlin module against the codegen'd spec and packages a debug APK with the module registered.

- [ ] **Step 8: Run the JS suite (unaffected by this task, confirms no regression)**

```bash
pnpm --filter mobile-app typecheck
pnpm --filter mobile-app test
```

Expected: unchanged pass count from Task 1.

- [ ] **Step 9: Commit**

```bash
git add apps/mobile-app/src/specs apps/mobile-app/package.json apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/MainApplication.kt
git commit -m "feat(mobile-app): scaffold AudioCapture TurboModule (stub implementation)"
```

---

### Task 3: Real capture engine (MediaProjection, AudioRecord, foreground service)

**Files:**
- Create: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngine.kt`
- Create: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureService.kt`
- Modify: `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureModule.kt` (replace stub bodies with real logic)
- Modify: `apps/mobile-app/android/app/src/main/AndroidManifest.xml`
- Modify: `apps/mobile-app/android/app/build.gradle` (add JUnit for the new JVM test)
- Create: `apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngineTest.kt`

**Interfaces:**
- Produces: `AudioCaptureEngine(mediaProjection, outputFile, onLevel)` with `start(sampleRate: Int)`, `pause()`, `resume()`, `stop(): Long` (returns the written file's byte size). Consumed only by `AudioCaptureModule` in this same task.
- `AudioCaptureModule`'s public TurboModule surface (method names/signatures) is unchanged from Task 2 — only method bodies change from stubs to real logic.

- [ ] **Step 1: Write the JVM unit test for the pure level-computation logic**

Create `apps/mobile-app/android/app/src/test/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngineTest.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import org.junit.Assert.assertEquals
import org.junit.Test

class AudioCaptureEngineTest {
  @Test
  fun `computeLevel returns zero for silence`() {
    val buffer = ShortArray(4) { 0 }
    assertEquals(0.0f, AudioCaptureEngine.computeLevel(buffer, 4), 0.0001f)
  }

  @Test
  fun `computeLevel returns close to one for full-scale samples`() {
    val buffer = shortArrayOf(32767, -32768, 32767, -32768)
    assertEquals(1.0f, AudioCaptureEngine.computeLevel(buffer, 4), 0.01f)
  }

  @Test
  fun `computeLevel only considers readCount samples`() {
    val buffer = shortArrayOf(32767, 32767, 0, 0)
    assertEquals(1.0f, AudioCaptureEngine.computeLevel(buffer, 2), 0.01f)
  }
}
```

- [ ] **Step 2: Add JUnit to the app's test dependencies**

In `apps/mobile-app/android/app/build.gradle`, add to the `dependencies` block:

```gradle
dependencies {
    // The version of react-native is set by the React Native Gradle Plugin
    implementation("com.facebook.react:react-android")

    if (hermesEnabled.toBoolean()) {
        implementation("com.facebook.react:hermes-android")
    } else {
        implementation jscFlavor
    }

    testImplementation "junit:junit:4.13.2"
}
```

(The RN template's `build.gradle` has no test dependencies at all by default — confirmed during planning: `testDebugUnitTest` fails with `Unresolved reference 'junit'` without this.)

- [ ] **Step 3: Run test to verify it fails**

```bash
cd apps/mobile-app/android
./gradlew :app:testDebugUnitTest --console=plain
```

Expected: FAIL — `AudioCaptureEngine` doesn't exist yet.

- [ ] **Step 4: Write `AudioCaptureEngine.kt`**

Create `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureEngine.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioPlaybackCaptureConfiguration
import android.media.AudioRecord
import android.media.projection.MediaProjection
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.sqrt

class AudioCaptureEngine(
  private val mediaProjection: MediaProjection,
  private val outputFile: File,
  private val onLevel: (Float) -> Unit,
) {
  companion object {
    private const val LEVEL_EMIT_INTERVAL_MS = 100L

    fun computeLevel(buffer: ShortArray, readCount: Int): Float {
      if (readCount == 0) return 0f
      var sumSquares = 0.0
      for (i in 0 until readCount) {
        val normalized = buffer[i] / 32768.0
        sumSquares += normalized * normalized
      }
      return sqrt(sumSquares / readCount).toFloat()
    }
  }

  private var audioRecord: AudioRecord? = null
  private var thread: Thread? = null
  private var outputStream: FileOutputStream? = null
  private val running = AtomicBoolean(false)
  private val paused = AtomicBoolean(false)

  fun start(sampleRate: Int) {
    val captureConfig =
      AudioPlaybackCaptureConfiguration.Builder(mediaProjection)
        .addMatchingUsage(AudioAttributes.USAGE_MEDIA)
        .addMatchingUsage(AudioAttributes.USAGE_GAME)
        .addMatchingUsage(AudioAttributes.USAGE_UNKNOWN)
        .build()

    val channelMask = AudioFormat.CHANNEL_IN_STEREO
    val encoding = AudioFormat.ENCODING_PCM_16BIT
    val minBufferSize = AudioRecord.getMinBufferSize(sampleRate, channelMask, encoding)
    val bufferSizeInBytes = if (minBufferSize > 0) minBufferSize * 2 else sampleRate * 2

    val audioFormat =
      AudioFormat.Builder()
        .setEncoding(encoding)
        .setSampleRate(sampleRate)
        .setChannelMask(channelMask)
        .build()

    val record =
      AudioRecord.Builder()
        .setAudioFormat(audioFormat)
        .setBufferSizeInBytes(bufferSizeInBytes)
        .setAudioPlaybackCaptureConfig(captureConfig)
        .build()

    audioRecord = record
    outputStream = FileOutputStream(outputFile)
    running.set(true)
    paused.set(false)
    record.startRecording()

    val readThread =
      Thread {
        val buffer = ShortArray(bufferSizeInBytes / 2)
        var lastEmitAt = 0L
        while (running.get()) {
          val readCount = record.read(buffer, 0, buffer.size)
          if (readCount > 0 && !paused.get()) {
            writeSamples(buffer, readCount)

            val now = System.currentTimeMillis()
            if (now - lastEmitAt >= LEVEL_EMIT_INTERVAL_MS) {
              onLevel(computeLevel(buffer, readCount))
              lastEmitAt = now
            }
          }
        }
      }
    thread = readThread
    readThread.start()
  }

  private fun writeSamples(buffer: ShortArray, readCount: Int) {
    val byteBuffer = ByteBuffer.allocate(readCount * 2).order(ByteOrder.LITTLE_ENDIAN)
    for (i in 0 until readCount) {
      byteBuffer.putShort(buffer[i])
    }
    outputStream?.write(byteBuffer.array())
  }

  fun pause() {
    paused.set(true)
  }

  fun resume() {
    paused.set(false)
  }

  fun stop(): Long {
    running.set(false)
    thread?.join(2000)
    thread = null
    audioRecord?.stop()
    audioRecord?.release()
    audioRecord = null
    outputStream?.flush()
    outputStream?.close()
    outputStream = null
    return outputFile.length()
  }
}
```

- [ ] **Step 5: Run the JVM unit test to verify it passes**

```bash
cd apps/mobile-app/android
./gradlew :app:testDebugUnitTest --console=plain
```

Expected: `BUILD SUCCESSFUL`, 3 tests passing (confirm via
`app/build/test-results/testDebugUnitTest/*.xml` showing `tests="3" failures="0" errors="0"`).

- [ ] **Step 6: Write `AudioCaptureService.kt`**

Create `apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture/AudioCaptureService.kt`:

```kotlin
package com.soundrecorder.mobile.audiocapture

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

class AudioCaptureService : Service() {
  companion object {
    private const val CHANNEL_ID = "audio_capture"
    private const val NOTIFICATION_ID = 1001

    fun start(context: Context) {
      val intent = Intent(context, AudioCaptureService::class.java)
      context.startForegroundService(intent)
    }

    fun stop(context: Context) {
      context.stopService(Intent(context, AudioCaptureService::class.java))
    }
  }

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    createNotificationChannel()
    val notification =
      Notification.Builder(this, CHANNEL_ID)
        .setContentTitle("Sound Recorder")
        .setContentText("Recording system audio")
        .setSmallIcon(android.R.drawable.ic_btn_speak_now)
        .build()

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
      startForeground(
        NOTIFICATION_ID,
        notification,
        ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION,
      )
    } else {
      startForeground(NOTIFICATION_ID, notification)
    }
    return START_NOT_STICKY
  }

  private fun createNotificationChannel() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    val manager = getSystemService(NotificationManager::class.java)
    val channel =
      NotificationChannel(CHANNEL_ID, "Audio Capture", NotificationManager.IMPORTANCE_LOW)
    manager.createNotificationChannel(channel)
  }
}
```

This service **only** manages the foreground notification Android requires for `MediaProjection` capture — it does not itself touch `AudioRecord`; `AudioCaptureModule` owns the actual capture engine directly. This is a deliberate simplification over having the service own the engine: cross-component (`Service` ↔ `Module`) communication for something as frequent as level events would add real complexity for no benefit here, since both live in the same process.

- [ ] **Step 7: Replace `AudioCaptureModule.kt`'s stub bodies with real logic**

Replace the file entirely with:

```kotlin
package com.soundrecorder.mobile.audiocapture

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioManager
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import androidx.core.content.ContextCompat
import com.facebook.react.bridge.ActivityEventListener
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.modules.core.DeviceEventManagerModule
import com.facebook.react.modules.core.PermissionAwareActivity
import com.facebook.react.modules.core.PermissionListener
import java.io.File

class AudioCaptureModule(private val reactContext: ReactApplicationContext) :
  NativeAudioCaptureSpec(reactContext),
  ActivityEventListener {

  companion object {
    const val NAME = "AudioCapture"
    private const val PROJECTION_REQUEST_CODE = 9001
    private const val RECORD_AUDIO_PERMISSION_REQUEST_CODE = 9002
    private const val DEFAULT_SAMPLE_RATE = 48000
    private const val TEMP_FILE_NAME = "recording.pcm.tmp"
  }

  init {
    reactContext.addActivityEventListener(this)
  }

  private var pendingStartPromise: Promise? = null
  private var mediaProjection: MediaProjection? = null
  private var engine: AudioCaptureEngine? = null

  override fun isSupported(promise: Promise) {
    promise.resolve(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
  }

  override fun listSources(promise: Promise) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
      promise.resolve(Arguments.createArray())
      return
    }
    val source = Arguments.createMap()
    source.putString("id", "system-audio")
    source.putString("name", "Device Audio")
    val sources = Arguments.createArray()
    sources.pushMap(source)
    promise.resolve(sources)
  }

  override fun startCapture(sourceId: String, promise: Promise) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
      promise.reject("UNSUPPORTED", "System audio recording requires Android 10 or later")
      return
    }
    val activity = reactContext.currentActivity
    if (activity == null) {
      promise.reject("NO_ACTIVITY", "No current activity to request capture permission from")
      return
    }
    pendingStartPromise = promise
    requestRecordAudioPermission(activity)
  }

  private fun requestRecordAudioPermission(activity: Activity) {
    if (ContextCompat.checkSelfPermission(reactContext, Manifest.permission.RECORD_AUDIO) ==
      PackageManager.PERMISSION_GRANTED
    ) {
      requestProjection(activity)
      return
    }
    val permissionAwareActivity = activity as? PermissionAwareActivity
    if (permissionAwareActivity == null) {
      failPendingStart("NO_PERMISSION_ACTIVITY", "Activity cannot request permissions")
      return
    }
    permissionAwareActivity.requestPermissions(
      arrayOf(Manifest.permission.RECORD_AUDIO),
      RECORD_AUDIO_PERMISSION_REQUEST_CODE,
      PermissionListener { requestCode, _, grantResults ->
        if (requestCode != RECORD_AUDIO_PERMISSION_REQUEST_CODE) {
          return@PermissionListener false
        }
        if (grantResults.isNotEmpty() && grantResults[0] == PackageManager.PERMISSION_GRANTED) {
          requestProjection(activity)
        } else {
          failPendingStart(
            "PERMISSION_DENIED",
            "Microphone permission (required by the system for playback capture) was denied",
          )
        }
        true
      },
    )
  }

  private fun requestProjection(activity: Activity) {
    val manager =
      reactContext.getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
    activity.startActivityForResult(manager.createScreenCaptureIntent(), PROJECTION_REQUEST_CODE)
  }

  override fun onActivityResult(
    activity: Activity,
    requestCode: Int,
    resultCode: Int,
    data: Intent?,
  ) {
    if (requestCode != PROJECTION_REQUEST_CODE) return
    if (resultCode != Activity.RESULT_OK || data == null) {
      failPendingStart("CAPTURE_DENIED", "System audio capture permission was denied")
      return
    }
    val manager =
      reactContext.getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
    val projection = manager.getMediaProjection(resultCode, data)
    if (projection == null) {
      failPendingStart("CAPTURE_FAILED", "Could not obtain media projection")
      return
    }
    mediaProjection = projection
    AudioCaptureService.start(reactContext)

    val audioManager = reactContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    val sampleRate =
      audioManager.getProperty(AudioManager.PROPERTY_OUTPUT_SAMPLE_RATE)?.toIntOrNull()
        ?: DEFAULT_SAMPLE_RATE

    val outputFile = File(reactContext.filesDir, TEMP_FILE_NAME)
    val captureEngine =
      AudioCaptureEngine(
        mediaProjection = projection,
        outputFile = outputFile,
        onLevel = { level -> emitLevel(level) },
      )
    engine = captureEngine
    captureEngine.start(sampleRate)

    pendingStartPromise?.resolve(null)
    pendingStartPromise = null
  }

  override fun onNewIntent(intent: Intent) {}

  private fun failPendingStart(code: String, message: String) {
    pendingStartPromise?.reject(code, message)
    pendingStartPromise = null
  }

  private fun emitLevel(level: Float) {
    reactContext
      .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
      .emit("AudioCaptureLevel", level.toDouble())
  }

  override fun pauseCapture() {
    engine?.pause()
  }

  override fun resumeCapture() {
    engine?.resume()
  }

  override fun stopCapture(promise: Promise) {
    val captureEngine = engine
    if (captureEngine == null) {
      promise.reject("NOT_RECORDING", "No active capture to stop")
      return
    }
    val sizeBytes = captureEngine.stop()
    engine = null
    mediaProjection?.stop()
    mediaProjection = null
    AudioCaptureService.stop(reactContext)

    val tempFile = File(reactContext.filesDir, TEMP_FILE_NAME)
    val finalFile = File(reactContext.filesDir, "recording-${System.currentTimeMillis()}.pcm")
    tempFile.renameTo(finalFile)

    val result = Arguments.createMap()
    result.putString("filePath", finalFile.absolutePath)
    result.putDouble("sizeBytes", sizeBytes.toDouble())
    promise.resolve(result)
  }

  override fun addListener(eventName: String) {}

  override fun removeListeners(count: Double) {}
}
```

Note the two non-nullable listener-interface signatures — `onActivityResult(activity: Activity, ...)` and `onNewIntent(intent: Intent)` take non-null parameters, not `Activity?`/`Intent?` (confirmed by compiler error during planning: using `?` there produces "overrides nothing"). Also note `reactContext.currentActivity` (called through the stored `reactContext` field), not a bare `currentActivity` reference — the bare form fails with "Function invocation 'getCurrentActivity()' expected" (confirmed during planning).

- [ ] **Step 8: Update `AndroidManifest.xml`**

In `apps/mobile-app/android/app/src/main/AndroidManifest.xml`, add the three permissions after the existing `INTERNET` permission, and the service declaration as the first child of `<application>`:

```xml
    <uses-permission android:name="android.permission.INTERNET" />
    <uses-permission android:name="android.permission.RECORD_AUDIO" />
    <uses-permission android:name="android.permission.FOREGROUND_SERVICE" />
    <uses-permission android:name="android.permission.FOREGROUND_SERVICE_MEDIA_PROJECTION" />

    <application
      android:name=".MainApplication"
      android:label="@string/app_name"
      android:icon="@mipmap/ic_launcher"
      android:roundIcon="@mipmap/ic_launcher_round"
      android:allowBackup="false"
      android:theme="@style/AppTheme"
      android:usesCleartextTraffic="${usesCleartextTraffic}"
      android:supportsRtl="true">
      <service
        android:name=".audiocapture.AudioCaptureService"
        android:enabled="true"
        android:exported="false"
        android:foregroundServiceType="mediaProjection" />
      <activity
```

(Keep everything else in the file — the `<activity>` block and its contents — unchanged; this only adds the three `<uses-permission>` lines and the `<service>` element immediately before the existing `<activity>` element.)

- [ ] **Step 9: Verify the full build**

```bash
cd apps/mobile-app/android
./gradlew assembleDebug --console=plain
./gradlew :app:testDebugUnitTest --console=plain
```

Expected: both `BUILD SUCCESSFUL`.

- [ ] **Step 10: Commit**

```bash
git add apps/mobile-app/android/app/src/main/java/com/soundrecorder/mobile/audiocapture apps/mobile-app/android/app/src/test apps/mobile-app/android/app/src/main/AndroidManifest.xml apps/mobile-app/android/app/build.gradle
git commit -m "feat(mobile-app): implement real AudioPlaybackCapture via MediaProjection + AudioRecord"
```

---

### Task 4: TS wrapper (`AndroidPlaybackCapture`)

**Files:**
- Create: `apps/mobile-app/src/capture/androidPlaybackCapture.ts`
- Create: `apps/mobile-app/src/capture/androidPlaybackCapture.test.ts`

**Interfaces:**
- Consumes: `NativeAudioCapture` default export from `../specs/NativeAudioCapture` (Task 2); `AudioCapture`/`AudioSource`/`CaptureResult` from `./types` (Task 1).
- Produces: `class AndroidPlaybackCapture implements AudioCapture`, consumed by Task 5's platform-selection change to `useRecordingState.ts`.

- [ ] **Step 1: Write the failing test**

```ts
import { DeviceEventEmitter } from "react-native"

jest.mock("../specs/NativeAudioCapture", () => ({
  __esModule: true,
  default: {
    isSupported: jest.fn(async () => true),
    listSources: jest.fn(async () => [
      { id: "system-audio", name: "Device Audio" },
    ]),
    startCapture: jest.fn(async () => undefined),
    pauseCapture: jest.fn(),
    resumeCapture: jest.fn(),
    stopCapture: jest.fn(async () => ({
      filePath: "/data/recording.pcm",
      sizeBytes: 4096,
    })),
    addListener: jest.fn(),
    removeListeners: jest.fn(),
  },
}))

import NativeAudioCapture from "../specs/NativeAudioCapture"
import { AndroidPlaybackCapture } from "./androidPlaybackCapture"

describe("AndroidPlaybackCapture", () => {
  afterEach(() => {
    jest.clearAllMocks()
  })

  it("returns sources from the native module when supported", async () => {
    const capture = new AndroidPlaybackCapture()
    const sources = await capture.listSources()
    expect(sources).toEqual([{ id: "system-audio", name: "Device Audio" }])
  })

  it("returns no sources when unsupported", async () => {
    ;(NativeAudioCapture.isSupported as jest.Mock).mockResolvedValueOnce(false)
    const capture = new AndroidPlaybackCapture()
    const sources = await capture.listSources()
    expect(sources).toEqual([])
  })

  it("forwards level events emitted by the native module to the onLevel callback", async () => {
    const capture = new AndroidPlaybackCapture()
    const levels: number[] = []
    await capture.start("system-audio", (level) => levels.push(level))

    DeviceEventEmitter.emit("AudioCaptureLevel", 0.42)

    expect(levels).toEqual([0.42])
    await capture.stop()
  })

  it("stops listening for level events after stop", async () => {
    const capture = new AndroidPlaybackCapture()
    const levels: number[] = []
    await capture.start("system-audio", (level) => levels.push(level))
    await capture.stop()

    DeviceEventEmitter.emit("AudioCaptureLevel", 0.9)

    expect(levels).toEqual([])
  })

  it("delegates pause/resume/stop to the native module", async () => {
    const capture = new AndroidPlaybackCapture()
    await capture.start("system-audio", () => {})

    capture.pause()
    capture.resume()
    const result = await capture.stop()

    expect(NativeAudioCapture.pauseCapture).toHaveBeenCalled()
    expect(NativeAudioCapture.resumeCapture).toHaveBeenCalled()
    expect(result).toEqual({ filePath: "/data/recording.pcm", sizeBytes: 4096 })
  })

  it("cleans up the level subscription if startCapture rejects", async () => {
    ;(NativeAudioCapture.startCapture as jest.Mock).mockRejectedValueOnce(
      new Error("capture denied")
    )
    const capture = new AndroidPlaybackCapture()
    const levels: number[] = []

    await expect(
      capture.start("system-audio", (level) => levels.push(level))
    ).rejects.toThrow("capture denied")

    DeviceEventEmitter.emit("AudioCaptureLevel", 0.5)
    expect(levels).toEqual([])
  })
})
```

Save as `apps/mobile-app/src/capture/androidPlaybackCapture.test.ts`.

This test relies on the standard RN testing pattern where `NativeEventEmitter(nativeModule)` subscribes through the shared `DeviceEventEmitter`/`RCTDeviceEventEmitter` regardless of which native module object was passed to its constructor (the module's `addListener`/`removeListeners` are only called for native-side reference counting). If `DeviceEventEmitter.emit(...)` does not reach a listener registered via `new NativeEventEmitter(NativeAudioCapture).addListener(...)` in this RN version, that is a real, worth-escalating surprise — stop and report NEEDS_CONTEXT with the exact failure rather than switching to a different mocking strategy on your own judgment.

- [ ] **Step 2: Run test to verify it fails**

Run: `pnpm --filter mobile-app test androidPlaybackCapture.test.ts`
Expected: FAIL — `androidPlaybackCapture.ts` doesn't exist yet.

- [ ] **Step 3: Write `androidPlaybackCapture.ts`**

```ts
import { NativeEventEmitter } from "react-native"

import NativeAudioCapture from "../specs/NativeAudioCapture"
import type { AudioCapture, AudioSource, CaptureResult } from "./types"

const LEVEL_EVENT = "AudioCaptureLevel"

export class AndroidPlaybackCapture implements AudioCapture {
  private emitter = new NativeEventEmitter(NativeAudioCapture)
  private subscription: { remove: () => void } | null = null

  async listSources(): Promise<AudioSource[]> {
    const supported = await NativeAudioCapture.isSupported()
    if (!supported) return []
    return NativeAudioCapture.listSources()
  }

  async start(
    sourceId: string,
    onLevel: (level: number) => void
  ): Promise<void> {
    this.subscription?.remove()
    this.subscription = this.emitter.addListener(LEVEL_EVENT, (level: number) => {
      onLevel(level)
    })
    try {
      await NativeAudioCapture.startCapture(sourceId)
    } catch (err) {
      this.subscription?.remove()
      this.subscription = null
      throw err
    }
  }

  pause(): void {
    NativeAudioCapture.pauseCapture()
  }

  resume(): void {
    NativeAudioCapture.resumeCapture()
  }

  async stop(): Promise<CaptureResult> {
    this.subscription?.remove()
    this.subscription = null
    return NativeAudioCapture.stopCapture()
  }
}
```

If TypeScript rejects passing `NativeAudioCapture` directly to `new NativeEventEmitter(...)` (a `Spec`-typed TurboModule vs. the constructor's expected native-module type), an explicit `NativeAudioCapture as unknown as Parameters<typeof NativeEventEmitter>[0]` cast at that one call site is an acceptable, narrow escape hatch — do not weaken the `AudioCapture`/`Spec` interfaces themselves to work around it.

- [ ] **Step 4: Run test to verify it passes**

Run: `pnpm --filter mobile-app test androidPlaybackCapture.test.ts`
Expected: PASS (6 tests)

- [ ] **Step 5: Run the full mobile-app suite**

```bash
pnpm --filter mobile-app typecheck
pnpm --filter mobile-app lint
pnpm --filter mobile-app test
```

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add apps/mobile-app/src/capture/androidPlaybackCapture.ts apps/mobile-app/src/capture/androidPlaybackCapture.test.ts
git commit -m "feat(mobile-app): add AndroidPlaybackCapture TS wrapper implementing AudioCapture"
```

---

### Task 5: Platform wiring + unsupported-OS empty state

**Files:**
- Modify: `apps/mobile-app/src/hooks/useRecordingState.ts`
- Modify: `apps/mobile-app/src/components/MainScreen.tsx`
- Modify: `apps/mobile-app/src/components/MainScreen.test.tsx`

**Interfaces:**
- Consumes: `AndroidPlaybackCapture` from `../capture/androidPlaybackCapture` (Task 4).
- No new interfaces produced — this task only changes which `AudioCapture` implementation is selected by default and adds one UI branch.

- [ ] **Step 1: Select `AndroidPlaybackCapture` on Android in `useRecordingState.ts`'s fallback**

In `apps/mobile-app/src/hooks/useRecordingState.ts`, change the imports and fallback construction:

```ts
import { Platform } from "react-native"

import type { AudioCapture, AudioSource } from "../capture/types"
import { AndroidPlaybackCapture } from "../capture/androidPlaybackCapture"
import { FakeCapture } from "../capture/fakeCapture"
import { errorMessage } from "../lib/errorMessage"
```

and:

```ts
  const fallbackRef = useRef<AudioCapture | null>(null)
  if (fallbackRef.current === null) {
    fallbackRef.current =
      Platform.OS === "android" ? new AndroidPlaybackCapture() : new FakeCapture()
  }
  const activeCapture = capture ?? fallbackRef.current
```

Everything else in the file is unchanged from Task 1.

- [ ] **Step 2: Write the failing test for the unsupported-OS empty state**

Add this test to `apps/mobile-app/src/components/MainScreen.test.tsx` (alongside the existing tests, using the same `makeMockCapture` helper already in the file):

```ts
  it("shows an explanatory message when no sources are available", async () => {
    const capture = makeMockCapture([])
    render(<MainScreen capture={capture} />)

    await waitFor(() =>
      expect(
        screen.getByText(/System audio recording requires Android 10 or later/)
      ).toBeTruthy()
    )
    expect(screen.queryByText("Record")).toBeNull()
  })
```

- [ ] **Step 3: Run test to verify it fails**

Run: `pnpm --filter mobile-app test MainScreen.test.tsx`
Expected: FAIL — `MainScreen` currently renders nothing extra when `sources` is empty.

- [ ] **Step 4: Add the empty-state branch to `MainScreen.tsx`**

In `apps/mobile-app/src/components/MainScreen.tsx`, add this block immediately after the error banner block and before the `canStart && sources.length > 0` source-row block:

```tsx
      {canStart && sources.length === 0 && (
        <Text style={styles.unavailable}>
          No recording source available. System audio recording requires
          Android 10 or later.
        </Text>
      )}
```

And add the corresponding style to the `StyleSheet.create` block, next to `saving`/`saved`:

```ts
  unavailable: { color: "#6b7280" },
```

- [ ] **Step 5: Run test to verify it passes**

Run: `pnpm --filter mobile-app test MainScreen.test.tsx`
Expected: PASS (all `MainScreen` tests, including the new one).

- [ ] **Step 6: Run the full mobile-app suite, typecheck, and lint**

```bash
pnpm --filter mobile-app typecheck
pnpm --filter mobile-app lint
pnpm --filter mobile-app test
```

Expected: all pass.

- [ ] **Step 7: Verify the Android build still succeeds**

```bash
cd apps/mobile-app/android
./gradlew assembleDebug --console=plain
```

Expected: `BUILD SUCCESSFUL` (this task only touched JS/TS files, but this confirms nothing about the JS bundle step broke the native build).

- [ ] **Step 8: Commit**

```bash
git add apps/mobile-app/src/hooks/useRecordingState.ts apps/mobile-app/src/components/MainScreen.tsx apps/mobile-app/src/components/MainScreen.test.tsx
git commit -m "feat(mobile-app): select real Android capture at runtime, add unsupported-OS empty state"
```

---

## After this plan

Real on-device verification — granting the `RECORD_AUDIO` permission, going through the `MediaProjection` consent dialog, confirming audio is actually captured and the foreground notification appears, and inspecting the resulting `.pcm` file — cannot be performed in this environment (no working Android emulator) and must happen on physical Android hardware (API 29+) before this sub-project is considered fully validated end-to-end. The next sub-project (proper WAV writer, atomic rename, storage validation, crash recovery, audio-focus/interruption handling) depends on this one being real-device-verified first, since it builds directly on the raw PCM file this plan produces.
