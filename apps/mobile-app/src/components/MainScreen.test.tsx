import React from "react"
import { Alert } from "react-native"
import { fireEvent, render, screen, waitFor } from "@testing-library/react-native"

import type { AudioCapture, AudioSource } from "../capture/types"
import { FakeCapture } from "../capture/fakeCapture"
import { MainScreen } from "./MainScreen"

function makeMockCapture(sources: AudioSource[]): AudioCapture {
  return {
    listSources: jest.fn(async () => sources),
    start: jest.fn(async () => {}),
    pause: jest.fn(),
    resume: jest.fn(),
    stop: jest.fn(async () => {}),
  }
}

describe("MainScreen", () => {
  it("shows the Record button and hides Pause/Stop/Cancel when idle", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    render(<MainScreen capture={capture} />)

    await waitFor(() => expect(screen.getByText("Source 1")).toBeTruthy())

    expect(screen.getByText("Record")).toBeTruthy()
    expect(screen.queryByText("Pause")).toBeNull()
    expect(screen.queryByText("Stop")).toBeNull()
    expect(screen.queryByText("Cancel")).toBeNull()
  })

  it("shows Pause/Stop/Cancel and hides Record after starting a recording", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    render(<MainScreen capture={capture} />)

    await waitFor(() => expect(screen.getByText("Source 1")).toBeTruthy())
    fireEvent.press(screen.getByText("Record"))

    await waitFor(() => expect(screen.getByText("Pause")).toBeTruthy())
    expect(screen.queryByText("Record")).toBeNull()
    expect(screen.getByText("Stop")).toBeTruthy()
    expect(screen.getByText("Cancel")).toBeTruthy()
  })

  it("confirms before cancelling and discards on confirmation", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    const alertSpy = jest.spyOn(Alert, "alert")
    render(<MainScreen capture={capture} />)

    await waitFor(() => expect(screen.getByText("Source 1")).toBeTruthy())
    fireEvent.press(screen.getByText("Record"))
    await waitFor(() => expect(screen.getByText("Cancel")).toBeTruthy())

    fireEvent.press(screen.getByText("Cancel"))

    expect(alertSpy).toHaveBeenCalled()
    const buttons = alertSpy.mock.calls[0][2]
    const discardButton = buttons?.find((b) => b.text === "Discard")

    await discardButton?.onPress?.()

    await waitFor(() => expect(screen.getByText("Record")).toBeTruthy())
  })

  it("uses a single stable FakeCapture instance when no capture prop is given", async () => {
    const listSourcesSpy = jest.spyOn(FakeCapture.prototype, "listSources")

    render(<MainScreen />)

    await waitFor(() => expect(screen.getByText("Fake System Audio")).toBeTruthy())

    expect(listSourcesSpy).toHaveBeenCalledTimes(1)

    listSourcesSpy.mockRestore()
  })
})
