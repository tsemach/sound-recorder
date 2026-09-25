import React from "react"
import { Alert } from "react-native"
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react-native"

import type { AudioCapture, AudioSource } from "../capture/types"
import { FakeCapture } from "../capture/fakeCapture"
import { MainScreen } from "./MainScreen"

// Mock the native AudioCapture module so tests don't try to load the TurboModule
jest.mock("../specs/NativeAudioCapture", () => ({
  getAudioCaptureNativeModule: () => ({
    isSupported: jest.fn(async () => false),
    listSources: jest.fn(async () => []),
    startCapture: jest.fn(async () => {}),
    pauseCapture: jest.fn(),
    resumeCapture: jest.fn(),
    stopCapture: jest.fn(async () => ({ filePath: "", sizeBytes: 0 })),
  }),
}))

function makeMockCapture(sources: AudioSource[]): AudioCapture {
  return {
    listSources: jest.fn(async () => sources),
    start: jest.fn(async () => {}),
    pause: jest.fn(),
    resume: jest.fn(),
    stop: jest.fn(async () => ({ filePath: "mock/recording.wav", sizeBytes: 1024 })),
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

  it("shows a saved confirmation after stopping", async () => {
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    render(<MainScreen capture={capture} />)

    await waitFor(() => expect(screen.getByText("Source 1")).toBeTruthy())
    fireEvent.press(screen.getByText("Record"))
    await waitFor(() => expect(screen.getByText("Stop")).toBeTruthy())

    fireEvent.press(screen.getByText("Stop"))

    await waitFor(() => expect(screen.getByText(/^Saved ·/)).toBeTruthy())
  })

  it("formats elapsed time while recording", async () => {
    jest.useFakeTimers()
    const capture = makeMockCapture([{ id: "s1", name: "Source 1" }])
    render(<MainScreen capture={capture} />)

    await waitFor(() => expect(screen.getByText("Source 1")).toBeTruthy())
    fireEvent.press(screen.getByText("Record"))
    await waitFor(() => expect(screen.getByText("Pause")).toBeTruthy())

    act(() => {
      jest.advanceTimersByTime(1000)
    })

    expect(screen.getByText("00:01")).toBeTruthy()

    jest.useRealTimers()
  })

  it("shows an error banner when loading sources fails", async () => {
    const capture: AudioCapture = {
      listSources: jest.fn(async () => {
        throw new Error("mic unavailable")
      }),
      start: jest.fn(async () => {}),
      pause: jest.fn(),
      resume: jest.fn(),
      stop: jest.fn(async () => ({ filePath: "mock/recording.wav", sizeBytes: 1024 })),
    }

    render(<MainScreen capture={capture} />)

    await waitFor(() =>
      expect(screen.getByText("mic unavailable")).toBeTruthy()
    )
  })

  it("uses a single stable FakeCapture instance when no capture prop is given", async () => {
    const listSourcesSpy = jest.spyOn(FakeCapture.prototype, "listSources")

    render(<MainScreen />)

    await waitFor(() =>
      expect(screen.getByText("Fake System Audio")).toBeTruthy()
    )

    expect(listSourcesSpy).toHaveBeenCalledTimes(1)

    listSourcesSpy.mockRestore()
  })

  it("shows an explanatory message when no sources are available", async () => {
    const capture = makeMockCapture([])
    render(<MainScreen capture={capture} />)

    await waitFor(() =>
      expect(
        screen.getByText(/System audio recording requires Android 10 or later/)
      ).toBeTruthy()
    )
    expect(screen.queryByText("Record")).toBeNull()
  })
})
