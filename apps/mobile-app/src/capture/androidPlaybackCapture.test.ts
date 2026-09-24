import { DeviceEventEmitter } from "react-native"

jest.mock("../specs/NativeAudioCapture", () => ({
  __esModule: true,
  default: {
    isSupported: jest.fn(async () => true),
    listSources: jest.fn(async () => [
      { id: "system-audio", name: "Device Audio" },
    ]),
    startCapture: jest.fn(async () => undefined),
    pauseCapture: jest.fn(),
    resumeCapture: jest.fn(),
    stopCapture: jest.fn(async () => ({
      filePath: "/data/recording.pcm",
      sizeBytes: 4096,
    })),
    addListener: jest.fn(),
    removeListeners: jest.fn(),
  },
}))

import NativeAudioCaptureModule from "../specs/NativeAudioCapture"
import { AndroidPlaybackCapture } from "./androidPlaybackCapture"

// The mock above always provides a non-null default export; assert that
// here so the tests can use it without repeating null-guards that only
// matter for the real, possibly-absent (e.g. on iOS), TurboModule.
const NativeAudioCapture = NativeAudioCaptureModule as NonNullable<
  typeof NativeAudioCaptureModule
>

describe("AndroidPlaybackCapture", () => {
  afterEach(() => {
    jest.clearAllMocks()
  })

  it("returns sources from the native module when supported", async () => {
    const capture = new AndroidPlaybackCapture()
    const sources = await capture.listSources()
    expect(sources).toEqual([{ id: "system-audio", name: "Device Audio" }])
  })

  it("returns no sources when unsupported", async () => {
    ;(NativeAudioCapture.isSupported as jest.Mock).mockResolvedValueOnce(false)
    const capture = new AndroidPlaybackCapture()
    const sources = await capture.listSources()
    expect(sources).toEqual([])
  })

  it("forwards level events emitted by the native module to the onLevel callback", async () => {
    const capture = new AndroidPlaybackCapture()
    const levels: number[] = []
    await capture.start("system-audio", (level) => levels.push(level))

    DeviceEventEmitter.emit("AudioCaptureLevel", 0.42)

    expect(levels).toEqual([0.42])
    await capture.stop()
  })

  it("stops listening for level events after stop", async () => {
    const capture = new AndroidPlaybackCapture()
    const levels: number[] = []
    await capture.start("system-audio", (level) => levels.push(level))
    await capture.stop()

    DeviceEventEmitter.emit("AudioCaptureLevel", 0.9)

    expect(levels).toEqual([])
  })

  it("delegates pause/resume/stop to the native module", async () => {
    const capture = new AndroidPlaybackCapture()
    await capture.start("system-audio", () => {})

    capture.pause()
    capture.resume()
    const result = await capture.stop()

    expect(NativeAudioCapture.pauseCapture).toHaveBeenCalled()
    expect(NativeAudioCapture.resumeCapture).toHaveBeenCalled()
    expect(result).toEqual({ filePath: "/data/recording.pcm", sizeBytes: 4096 })
  })

  it("cleans up the level subscription if startCapture rejects", async () => {
    ;(NativeAudioCapture.startCapture as jest.Mock).mockRejectedValueOnce(
      new Error("capture denied")
    )
    const capture = new AndroidPlaybackCapture()
    const levels: number[] = []

    await expect(
      capture.start("system-audio", (level) => levels.push(level))
    ).rejects.toThrow("capture denied")

    DeviceEventEmitter.emit("AudioCaptureLevel", 0.5)
    expect(levels).toEqual([])
  })
})
