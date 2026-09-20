import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, it, vi } from "vitest"

import { App } from "./App"

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue("pong"),
}))

describe("App", () => {
  it("renders the ready message", () => {
    render(<App />)
    expect(screen.getByText("Project ready!")).toBeInTheDocument()
  })

  it("toggles dark mode when the d key is pressed", async () => {
    render(<App />)
    fireEvent.keyDown(window, { key: "d" })
    await waitFor(() =>
      expect(document.documentElement).toHaveClass("dark")
    )
  })

  it("pings the backend and displays the result", async () => {
    render(<App />)
    fireEvent.click(screen.getByRole("button", { name: "Ping backend" }))
    await waitFor(() => expect(screen.getByText("pong")).toBeInTheDocument())
  })
})
