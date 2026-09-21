import { render, screen, waitFor } from "@testing-library/react"
import { fireEvent } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { invoke } from "@tauri-apps/api/core"
import { revealItemInDir } from "@tauri-apps/plugin-opener"

import { RecordingsList } from "./RecordingsList"

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${path}`,
}))

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: vi.fn(),
}))

const mockedInvoke = invoke as unknown as ReturnType<typeof vi.fn>
const mockedRevealItemInDir = revealItemInDir as unknown as ReturnType<
  typeof vi.fn
>

const sampleRecording = {
  path: "/home/user/Music/Sound Recorder/recording-a.wav",
  filename: "recording-a.wav",
  created_at_ms: 1_700_000_000_000,
  duration_ms: 65_000,
  size_bytes: 2 * 1024 * 1024,
  format: "WAV 48 kHz · 2 ch · 16-bit",
}

describe("RecordingsList", () => {
  beforeEach(() => {
    mockedInvoke.mockReset()
    mockedRevealItemInDir.mockReset()
  })

  it("renders a fetched recording with its duration and size", async () => {
    mockedInvoke.mockResolvedValueOnce([sampleRecording])

    render(<RecordingsList />)

    await waitFor(() => {
      expect(screen.getByText("recording-a.wav")).toBeInTheDocument()
    })
    expect(screen.getByText(/01:05/)).toBeInTheDocument()
    expect(screen.getByText(/2\.0 MB/)).toBeInTheDocument()
  })

  it("shows an empty state when there are no recordings", async () => {
    mockedInvoke.mockResolvedValueOnce([])

    render(<RecordingsList />)

    await waitFor(() => {
      expect(screen.getByText("No recordings yet.")).toBeInTheDocument()
    })
  })

  it("calls delete_recording after confirming, then refreshes the list", async () => {
    mockedInvoke.mockResolvedValueOnce([sampleRecording])
    mockedInvoke.mockResolvedValueOnce(undefined)
    mockedInvoke.mockResolvedValueOnce([])
    vi.spyOn(window, "confirm").mockReturnValue(true)

    render(<RecordingsList />)
    await waitFor(() => {
      expect(screen.getByText("recording-a.wav")).toBeInTheDocument()
    })

    fireEvent.click(
      screen.getByRole("button", { name: "Delete recording-a.wav" })
    )

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("delete_recording", {
        name: "recording-a.wav",
      })
    })
    await waitFor(() => {
      expect(screen.getByText("No recordings yet.")).toBeInTheDocument()
    })
  })

  it("does not call delete_recording when the confirmation is declined", async () => {
    mockedInvoke.mockResolvedValueOnce([sampleRecording])
    vi.spyOn(window, "confirm").mockReturnValue(false)

    render(<RecordingsList />)
    await waitFor(() => {
      expect(screen.getByText("recording-a.wav")).toBeInTheDocument()
    })

    fireEvent.click(
      screen.getByRole("button", { name: "Delete recording-a.wav" })
    )

    expect(mockedInvoke).not.toHaveBeenCalledWith(
      "delete_recording",
      expect.anything()
    )
  })

  it("renames a recording and refreshes the list", async () => {
    mockedInvoke.mockResolvedValueOnce([sampleRecording])
    mockedInvoke.mockResolvedValueOnce(undefined)
    mockedInvoke.mockResolvedValueOnce([])

    render(<RecordingsList />)
    await waitFor(() => {
      expect(screen.getByText("recording-a.wav")).toBeInTheDocument()
    })

    fireEvent.click(
      screen.getByRole("button", { name: "Rename recording-a.wav" })
    )

    const input = screen.getByLabelText("New name for recording-a.wav")
    fireEvent.change(input, { target: { value: "renamed.wav" } })
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("rename_recording", {
        oldName: "recording-a.wav",
        newName: "renamed.wav",
      })
    })
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("list_recordings")
    })
  })

  it("reveals a recording in the file manager", async () => {
    mockedInvoke.mockResolvedValueOnce([sampleRecording])

    render(<RecordingsList />)
    await waitFor(() => {
      expect(screen.getByText("recording-a.wav")).toBeInTheDocument()
    })

    fireEvent.click(
      screen.getByRole("button", { name: "Reveal recording-a.wav" })
    )

    await waitFor(() => {
      expect(mockedRevealItemInDir).toHaveBeenCalledWith(sampleRecording.path)
    })
  })
})
