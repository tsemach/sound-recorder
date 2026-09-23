import { act, renderHook, waitFor } from "@testing-library/react-native"

import type { AudioCapture, AudioSource } from "../capture/types"
import { useRecordingState } from "./useRecordingState"

function makeMockCapture(sources: AudioSource[]): AudioCapture & {
  emitFrame: (frame: Int16Array) => void
} {
  let onFrame: ((frame: Int16Array) => void) | null = null
  return {
    listSources: jest.fn(async () => sources),
    start: jest.fn(async (_sourceId: string, cb: (frame: Int16Array) => void) => {
      onFrame = cb
    }),
    pause: jest.fn(),
    resume: jest.fn(),
    stop: jest.fn(async () => {
      onFrame = null
    }),
    emitFrame(frame: Int16Array) {
      onFrame?.(frame)
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

  it("updates elapsedMs and level from the tick loop while recording", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    act(() => {
      capture.emitFrame(new Int16Array([32767, -32768, 0, 0]))
      jest.advanceTimersByTime(100)
    })

    expect(result.current.elapsedMs).toBeGreaterThanOrEqual(100)
    expect(result.current.level).toBeGreaterThan(0)
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

  it("stopRecording moves Recording to Saved", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const { result } = renderHook(() => useRecordingState(capture))
    await waitFor(() => expect(result.current.sources).toHaveLength(1))

    await act(async () => {
      await result.current.startRecording("s1")
    })

    await act(async () => {
      await result.current.stopRecording()
    })

    expect(result.current.state.state).toBe("Saved")
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
