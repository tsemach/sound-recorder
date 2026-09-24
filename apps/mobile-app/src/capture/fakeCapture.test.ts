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

  it("reports levels roughly every 20ms while running", async () => {
    const capture = new FakeCapture()
    const levels: number[] = []
    await capture.start("fake-system-audio", (level) => levels.push(level))

    jest.advanceTimersByTime(100)

    expect(levels.length).toBeGreaterThanOrEqual(4)
    expect(levels[0]).toBeGreaterThan(0)

    await capture.stop()
  })

  it("stops reporting levels while paused", async () => {
    const capture = new FakeCapture()
    const levels: number[] = []
    await capture.start("fake-system-audio", (level) => levels.push(level))

    jest.advanceTimersByTime(40)
    capture.pause()
    const countAtPause = levels.length

    jest.advanceTimersByTime(100)
    expect(levels.length).toBe(countAtPause)

    await capture.stop()
  })

  it("resumes reporting levels after resume", async () => {
    const capture = new FakeCapture()
    const levels: number[] = []
    await capture.start("fake-system-audio", (level) => levels.push(level))

    jest.advanceTimersByTime(40)
    capture.pause()
    const countAtPause = levels.length
    capture.resume()
    jest.advanceTimersByTime(100)

    expect(levels.length).toBeGreaterThan(countAtPause)

    await capture.stop()
  })

  it("stops reporting levels entirely after stop", async () => {
    const capture = new FakeCapture()
    const levels: number[] = []
    await capture.start("fake-system-audio", (level) => levels.push(level))

    jest.advanceTimersByTime(40)
    await capture.stop()
    const countAtStop = levels.length

    jest.advanceTimersByTime(100)
    expect(levels.length).toBe(countAtStop)
  })

  it("throws if start() is called again while already running", async () => {
    const capture = new FakeCapture()
    await capture.start("fake-system-audio", () => {})

    await expect(capture.start("fake-system-audio", () => {})).rejects.toThrow(
      "FakeCapture.start() called while already running"
    )

    await capture.stop()
  })

  it("resolves stop() with a fake file path and zero size", async () => {
    const capture = new FakeCapture()
    await capture.start("fake-system-audio", () => {})

    const result = await capture.stop()

    expect(result.filePath).toMatch(/^fake\/recording-\d+\.wav$/)
    expect(result.sizeBytes).toBe(0)
  })
})
