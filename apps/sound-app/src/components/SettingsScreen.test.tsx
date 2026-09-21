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

  it("renders the given settings", () => {
    render(
      <SettingsScreen settings={defaultSettings} onSettingsChange={vi.fn()} />
    )

    expect(screen.getByText("Default location")).toBeInTheDocument()
    expect(screen.getByLabelText("Filename prefix")).toHaveValue("recording")
  })

  it("shows a custom save directory when one is set", () => {
    render(
      <SettingsScreen
        settings={{ ...defaultSettings, save_dir: "/home/user/MyRecordings" }}
        onSettingsChange={vi.fn()}
      />
    )

    expect(screen.getByText("/home/user/MyRecordings")).toBeInTheDocument()
  })

  it("picks a folder via the native dialog and persists it", async () => {
    mockedOpen.mockResolvedValueOnce("/home/user/Podcasts")
    mockedInvoke.mockResolvedValueOnce(undefined)
    const onSettingsChange = vi.fn()

    render(
      <SettingsScreen
        settings={defaultSettings}
        onSettingsChange={onSettingsChange}
      />
    )

    fireEvent.click(screen.getByRole("button", { name: "Choose Folder…" }))

    await waitFor(() => {
      expect(mockedOpen).toHaveBeenCalledWith({ directory: true })
    })
    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("update_settings", {
        settings: { ...defaultSettings, save_dir: "/home/user/Podcasts" },
      })
    })
    await waitFor(() => {
      expect(onSettingsChange).toHaveBeenCalledWith({
        ...defaultSettings,
        save_dir: "/home/user/Podcasts",
      })
    })
  })

  it("does not persist when the folder dialog is cancelled", async () => {
    mockedOpen.mockResolvedValueOnce(null)
    const onSettingsChange = vi.fn()

    render(
      <SettingsScreen
        settings={defaultSettings}
        onSettingsChange={onSettingsChange}
      />
    )

    fireEvent.click(screen.getByRole("button", { name: "Choose Folder…" }))

    await waitFor(() => {
      expect(mockedOpen).toHaveBeenCalled()
    })
    expect(mockedInvoke).not.toHaveBeenCalledWith(
      "update_settings",
      expect.anything()
    )
    expect(onSettingsChange).not.toHaveBeenCalled()
  })

  it("saves an edited filename prefix", async () => {
    mockedInvoke.mockResolvedValueOnce(undefined)
    const onSettingsChange = vi.fn()

    render(
      <SettingsScreen
        settings={defaultSettings}
        onSettingsChange={onSettingsChange}
      />
    )

    const input = screen.getByLabelText("Filename prefix")
    fireEvent.change(input, { target: { value: "meeting" } })
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("update_settings", {
        settings: { ...defaultSettings, filename_prefix: "meeting" },
      })
    })
    await waitFor(() => {
      expect(onSettingsChange).toHaveBeenCalledWith({
        ...defaultSettings,
        filename_prefix: "meeting",
      })
    })
  })

  it("shows the privacy note", () => {
    render(
      <SettingsScreen settings={defaultSettings} onSettingsChange={vi.fn()} />
    )

    expect(
      screen.getByText(/Recordings stay on this device/)
    ).toBeInTheDocument()
  })

  it("surfaces an error when persisting fails", async () => {
    mockedInvoke.mockRejectedValueOnce({
      message: "Filename prefix cannot be empty",
      recoverable: true,
    })
    const onSettingsChange = vi.fn()

    render(
      <SettingsScreen
        settings={defaultSettings}
        onSettingsChange={onSettingsChange}
      />
    )

    const input = screen.getByLabelText("Filename prefix")
    fireEvent.change(input, { target: { value: "" } })
    fireEvent.click(screen.getByRole("button", { name: "Save" }))

    await waitFor(() => {
      expect(
        screen.getByText("Filename prefix cannot be empty")
      ).toBeInTheDocument()
    })
    expect(onSettingsChange).not.toHaveBeenCalled()
  })
})
