import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { invoke } from "@tauri-apps/api/core"

import { App } from "./App"
import { useRecordingState } from "./hooks/useRecordingState"
import type { RecordingState } from "./hooks/useRecordingState"

vi.mock("./hooks/useRecordingState")

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}))

const mockUseRecordingState = vi.mocked(useRecordingState)
const mockedInvoke = invoke as unknown as ReturnType<typeof vi.fn>

function baseHookReturn(
  overrides: Partial<ReturnType<typeof useRecordingState>> = {}
): ReturnType<typeof useRecordingState> {
  return {
    state: { state: "Idle" },
    elapsedMs: 0,
    level: 0,
    sources: [{ id: "fake-system-audio", name: "Fake System Audio" }],
    error: null,
    startRecording: vi.fn(),
    pauseRecording: vi.fn(),
    resumeRecording: vi.fn(),
    stopRecording: vi.fn(),
    cancelRecording: vi.fn(),
    ...overrides,
  }
}

describe("App", () => {
  beforeEach(() => {
    mockUseRecordingState.mockReset()
    mockedInvoke.mockReset()
    mockedInvoke.mockResolvedValue({
      save_dir: null,
      filename_prefix: "",
      default_source_id: null,
    })
  })

  it("toggles dark mode when the d key is pressed", async () => {
    mockUseRecordingState.mockReturnValue(baseHookReturn())
    render(<App />)
    fireEvent.keyDown(window, { key: "d" })
    await waitFor(() => expect(document.documentElement).toHaveClass("dark"))
  })

  it("shows Record button and source select when idle", () => {
    mockUseRecordingState.mockReturnValue(baseHookReturn())
    render(<App />)
    expect(screen.getByRole("button", { name: "Record" })).toBeInTheDocument()
    expect(screen.getByRole("combobox")).toBeInTheDocument()
  })

  it("calls startRecording with the selected source when Record is clicked", () => {
    const startRecording = vi.fn()
    mockUseRecordingState.mockReturnValue(baseHookReturn({ startRecording }))
    render(<App />)
    fireEvent.click(screen.getByRole("button", { name: "Record" }))
    expect(startRecording).toHaveBeenCalledWith("fake-system-audio")
  })

  it("shows Pause, Stop, Cancel (not Record) while recording", () => {
    const recording: RecordingState = {
      state: "Recording",
      source_name: "Fake System Audio",
      elapsed_ms: 5000,
    }
    mockUseRecordingState.mockReturnValue(baseHookReturn({ state: recording }))
    render(<App />)
    expect(screen.getByRole("button", { name: "Pause" })).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument()
    expect(screen.getByRole("button", { name: "Cancel" })).toBeInTheDocument()
    expect(
      screen.queryByRole("button", { name: "Record" })
    ).not.toBeInTheDocument()
  })

  it("shows Resume while paused", () => {
    const paused: RecordingState = {
      state: "Paused",
      source_name: "Fake System Audio",
      elapsed_ms: 5000,
    }
    mockUseRecordingState.mockReturnValue(baseHookReturn({ state: paused }))
    render(<App />)
    expect(screen.getByRole("button", { name: "Resume" })).toBeInTheDocument()
  })

  it("formats elapsed time as mm:ss", () => {
    const recording: RecordingState = {
      state: "Recording",
      source_name: "Fake",
      elapsed_ms: 65000,
    }
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({ state: recording, elapsedMs: 65000 })
    )
    render(<App />)
    expect(screen.getByText("01:05")).toBeInTheDocument()
  })

  it("renders the error banner when present", () => {
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({
        error: "Cannot start recording from the current state",
      })
    )
    render(<App />)
    expect(
      screen.getByText("Cannot start recording from the current state")
    ).toBeInTheDocument()
  })

  it("confirms before cancelling", () => {
    const cancelRecording = vi.fn()
    vi.spyOn(window, "confirm").mockReturnValue(true)
    const recording: RecordingState = {
      state: "Recording",
      source_name: "Fake",
      elapsed_ms: 1000,
    }
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({ state: recording, cancelRecording })
    )
    render(<App />)
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    expect(window.confirm).toHaveBeenCalled()
    expect(cancelRecording).toHaveBeenCalled()
  })

  it("does not cancel when the confirmation is dismissed", () => {
    const cancelRecording = vi.fn()
    vi.spyOn(window, "confirm").mockReturnValue(false)
    const recording: RecordingState = {
      state: "Recording",
      source_name: "Fake",
      elapsed_ms: 1000,
    }
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({ state: recording, cancelRecording })
    )
    render(<App />)
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }))
    expect(window.confirm).toHaveBeenCalled()
    expect(cancelRecording).not.toHaveBeenCalled()
  })

  it("does not show the Record button when there are no sources", () => {
    mockUseRecordingState.mockReturnValue(baseHookReturn({ sources: [] }))
    render(<App />)
    expect(
      screen.queryByRole("button", { name: "Record" })
    ).not.toBeInTheDocument()
  })

  it("shows a Saving indicator while state is Saving", () => {
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({ state: { state: "Saving" } })
    )
    render(<App />)
    expect(screen.getByText("Saving…")).toBeInTheDocument()
  })

  it("does not show the Saving indicator outside the Saving state", () => {
    mockUseRecordingState.mockReturnValue(baseHookReturn())
    render(<App />)
    expect(screen.queryByText("Saving…")).not.toBeInTheDocument()
  })

  it("navigates to Settings and back to the recorder", async () => {
    mockUseRecordingState.mockReturnValue(baseHookReturn())
    render(<App />)

    fireEvent.click(screen.getByRole("button", { name: "Settings" }))
    await waitFor(() => {
      expect(screen.getByText("Save location")).toBeInTheDocument()
    })
    expect(
      screen.queryByRole("button", { name: "Record" })
    ).not.toBeInTheDocument()

    fireEvent.click(screen.getByRole("button", { name: "Back to Recorder" }))
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Record" })).toBeInTheDocument()
    })
  })

  it("pre-selects the persisted default source once settings load", async () => {
    mockUseRecordingState.mockReturnValue(
      baseHookReturn({
        sources: [
          { id: "fake-system-audio", name: "Fake System Audio" },
          { id: "other-source", name: "Other Source" },
        ],
      })
    )
    mockedInvoke.mockResolvedValue({
      save_dir: null,
      filename_prefix: "",
      default_source_id: "other-source",
    })

    render(<App />)

    await waitFor(() => {
      expect(screen.getByRole("combobox")).toHaveValue("other-source")
    })
  })
})
