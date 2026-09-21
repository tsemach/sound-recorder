import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { invoke } from "@tauri-apps/api/core"
import { open } from "@tauri-apps/plugin-dialog"

import { SettingsScreen } from "./SettingsScreen"

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}))

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}))

const mockedInvoke = invoke as unknown as ReturnType<typeof vi.fn>
const mockedOpen = open as unknown as ReturnType<typeof vi.fn>

const defaultSettings = {
  save_dir: null,
  filename_prefix: "recording",
  default_source_id: null,
}

describe("SettingsScreen", () => {
  beforeEach(() => {
    mockedInvoke.mockReset()
    mockedOpen.mockReset()
  })

  it("renders fetched settings", async () => {
    mockedInvoke.mockResolvedValueOnce(defaultSettings)

    render(<SettingsScreen />)

    await waitFor(() => {
      expect(screen.getByText("Default location")).toBeInTheDocument()
    })
    expect(screen.getByLabelText("Filename prefix")).toHaveValue("recording")
  })

  it("shows a custom save directory when one is set", async () => {
    mockedInvoke.mockResolvedValueOnce({
      ...defaultSettings,
      save_dir: "/home/user/MyRecordings",
    })

    render(<SettingsScreen />)

    await waitFor(() => {
      expect(screen.getByText("/home/user/MyRecordings")).toBeInTheDocument()
    })
  })

  it("picks a folder via the native dialog and persists it", async () => {
    mockedInvoke.mockResolvedValueOnce(defaultSettings)
    mockedOpen.mockResolvedValueOnce("/home/user/Podcasts")
    mockedInvoke.mockResolvedValueOnce(undefined)

    render(<SettingsScreen />)
    await waitFor(() => {
      expect(screen.getByText("Default location")).toBeInTheDocument()
    })

    fireEvent.click(screen.getByRole("button", { name: "Choose Folder…" }))

    await waitFor(() => {
      expect(mockedOpen).toHaveBeenCalledWith({ directory: true })
    })
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("update_settings", {
        settings: { ...defaultSettings, save_dir: "/home/user/Podcasts" },
      })
    })
  })

  it("does not persist when the folder dialog is cancelled", async () => {
    mockedInvoke.mockResolvedValueOnce(defaultSettings)
    mockedOpen.mockResolvedValueOnce(null)

    render(<SettingsScreen />)
    await waitFor(() => {
      expect(screen.getByText("Default location")).toBeInTheDocument()
    })

    fireEvent.click(screen.getByRole("button", { name: "Choose Folder…" }))

    await waitFor(() => {
      expect(mockedOpen).toHaveBeenCalled()
    })
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      "update_settings",
      expect.anything()
    )
  })

  it("saves an edited filename prefix", async () => {
    mockedInvoke.mockResolvedValueOnce(defaultSettings)
    mockedInvoke.mockResolvedValueOnce(undefined)

    render(<SettingsScreen />)
    await waitFor(() => {
      expect(screen.getByLabelText("Filename prefix")).toHaveValue("recording")
    })

    const input = screen.getByLabelText("Filename prefix")
    fireEvent.change(input, { target: { value: "meeting" } })
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("update_settings", {
        settings: { ...defaultSettings, filename_prefix: "meeting" },
      })
    })
  })

  it("shows the privacy note", async () => {
    mockedInvoke.mockResolvedValueOnce(defaultSettings)

    render(<SettingsScreen />)

    await waitFor(() => {
      expect(
        screen.getByText(/Recordings stay on this device/)
      ).toBeInTheDocument()
    })
  })
})
