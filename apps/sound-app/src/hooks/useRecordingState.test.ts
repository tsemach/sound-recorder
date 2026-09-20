import { act, renderHook, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

const { listeners, mockInvoke } = vi.hoisted(() => ({
  listeners: {} as Record<string, (event: { payload: unknown }) => void>,
  mockInvoke: vi.fn(),
}))

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}))

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(
    (
      event: string,
      callback: (event: { payload: unknown }) => void
    ) => {
      listeners[event] = callback
      return Promise.resolve(() => {
        delete listeners[event]
      })
    }
  ),
}))

import { useRecordingState } from "./useRecordingState"

describe("useRecordingState", () => {
  beforeEach(() => {
    mockInvoke.mockReset()
    mockInvoke.mockResolvedValue([
      { id: "fake-system-audio", name: "Fake System Audio" },
    ])
  })

  it("loads sources on mount", async () => {
    const { result } = renderHook(() => useRecordingState())
    await waitFor(() => expect(result.current.sources).toHaveLength(1))
    expect(mockInvoke).toHaveBeenCalledWith("list_sources")
  })

  it("updates state when a recording-state-changed event arrives", async () => {
    const { result } = renderHook(() => useRecordingState())
    await waitFor(() =>
      expect(listeners["recording-state-changed"]).toBeDefined()
    )

    act(() => {
      listeners["recording-state-changed"]!({
        payload: {
          state: "Recording",
          source_name: "Fake System Audio",
          elapsed_ms: 0,
        },
      })
    })

    expect(result.current.state).toEqual({
      state: "Recording",
      source_name: "Fake System Audio",
      elapsed_ms: 0,
    })
  })

  it("updates elapsedMs and level when a recording-tick event arrives", async () => {
    const { result } = renderHook(() => useRecordingState())
    await waitFor(() => expect(listeners["recording-tick"]).toBeDefined())

    act(() => {
      listeners["recording-tick"]!({
        payload: { elapsed_ms: 4200, level: 0.5 },
      })
    })

    expect(result.current.elapsedMs).toBe(4200)
    expect(result.current.level).toBe(0.5)
  })

  it("sets error when a command rejects", async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === "list_sources") return Promise.resolve([])
      if (cmd === "pause_recording") {
        return Promise.reject({
          message: "Cannot pause unless recording",
          recoverable: true,
        })
      }
      return Promise.resolve()
    })

    const { result } = renderHook(() => useRecordingState())
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("list_sources"))

    await act(async () => {
      await result.current.pauseRecording()
    })

    expect(result.current.error).toBe("Cannot pause unless recording")
  })
})
