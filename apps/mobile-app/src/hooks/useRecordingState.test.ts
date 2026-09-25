import { act, renderHook, waitFor } from "@testing-library/react-native"

import type { AudioCapture, AudioSource, CaptureResult } from "../capture/types"
import { useRecordingState } from "./useRecordingState"

// Mock the native AudioCapture module so tests don't try to load the TurboModule
jest.mock("../specs/NativeAudioCapture", () => ({
  getAudioCaptureNativeModule: () => ({
    isSupported: jest.fn(async () => false),
    listSources: jest.fn(async () => []),
    startCapture: jest.fn(async () => {}),
    pauseCapture: jest.fn(),
    resumeCapture: jest.fn(),
    stopCapture: jest.fn(async () => ({ filePath: "", sizeBytes: 0 })),
    discardCapture: jest.fn(async () => undefined),
  }),
}))

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
    discard: jest.fn(async () => {
      onLevel = null
    }),
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
    expect(capture.discard).toHaveBeenCalled()
    expect(capture.stop).not.toHaveBeenCalled()
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

  it("falls back to FakeCapture on a non-Android platform when no capture is given", async () => {
    // This test environment's Platform.OS defaults to "ios" (see
    // @react-native/jest-preset's haste.defaultPlatform), so rendering the
    // hook with no capture prop exercises the non-Android branch of the
    // fallback in useRecordingState. Asserting on FakeCapture's two known
    // source ids is sufficient proof it picked FakeCapture over
    // AndroidPlaybackCapture, without needing to mock Platform.OS directly.
    const { result } = renderHook(() => useRecordingState())

    await waitFor(() =>
      expect(result.current.sources).toEqual([
        { id: "fake-system-audio", name: "Fake System Audio" },
        { id: "fake-microphone", name: "Fake Microphone" },
      ])
    )
  })
})
