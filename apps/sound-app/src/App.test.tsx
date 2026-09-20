import { fireEvent, render, screen, waitFor } from "@testing-library/react"
import { describe, expect, it } from "vitest"

import { App } from "./App"

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
})
