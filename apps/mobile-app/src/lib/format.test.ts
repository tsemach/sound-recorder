import { formatDuration } from "./format"

describe("formatDuration", () => {
  it("formats under a minute as mm:ss", () => {
    expect(formatDuration(5000)).toBe("00:05")
  })

  it("formats minutes and seconds", () => {
    expect(formatDuration(65000)).toBe("01:05")
  })

  it("formats an hour or more as hh:mm:ss", () => {
    expect(formatDuration(3661000)).toBe("01:01:01")
  })

  it("formats zero as 00:00", () => {
    expect(formatDuration(0)).toBe("00:00")
  })
})
