import { FakeCapture } from "./fakeCapture"

describe("FakeCapture", () => {
  beforeEach(() => {
    jest.useFakeTimers()
  })

  afterEach(() => {
    jest.useRealTimers()
  })

  it("lists two fake sources", async () => {
    const capture = new FakeCapture()
    const sources = await capture.listSources()
    expect(sources).toEqual([
      { id: "fake-system-audio", name: "Fake System Audio" },
      { id: "fake-microphone", name: "Fake Microphone" },
    ])
  })

  it("produces frames roughly every 20ms while running", async () => {
    const capture = new FakeCapture()
    const frames: Int16Array[] = []
    await capture.start("fake-system-audio", (frame) => frames.push(frame))

    jest.advanceTimersByTime(100)

    expect(frames.length).toBeGreaterThanOrEqual(4)
    expect(frames[0].length).toBeGreaterThan(0)

    await capture.stop()
  })

  it("stops producing frames while paused", async () => {
    const capture = new FakeCapture()
    const frames: Int16Array[] = []
    await capture.start("fake-system-audio", (frame) => frames.push(frame))

    jest.advanceTimersByTime(40)
    capture.pause()
    const countAtPause = frames.length

    jest.advanceTimersByTime(100)
    expect(frames.length).toBe(countAtPause)

    await capture.stop()
  })

  it("resumes producing frames after resume", async () => {
    const capture = new FakeCapture()
    const frames: Int16Array[] = []
    await capture.start("fake-system-audio", (frame) => frames.push(frame))

    jest.advanceTimersByTime(40)
    capture.pause()
    const countAtPause = frames.length
    capture.resume()
    jest.advanceTimersByTime(100)

    expect(frames.length).toBeGreaterThan(countAtPause)

    await capture.stop()
  })

  it("stops producing frames entirely after stop", async () => {
    const capture = new FakeCapture()
    const frames: Int16Array[] = []
    await capture.start("fake-system-audio", (frame) => frames.push(frame))

    jest.advanceTimersByTime(40)
    await capture.stop()
    const countAtStop = frames.length

    jest.advanceTimersByTime(100)
    expect(frames.length).toBe(countAtStop)
  })

  it("throws if start() is called again while already running", async () => {
    const capture = new FakeCapture()
    await capture.start("fake-system-audio", () => {})

    await expect(capture.start("fake-system-audio", () => {})).rejects.toThrow(
      "FakeCapture.start() called while already running"
    )

    await capture.stop()
  })
})
