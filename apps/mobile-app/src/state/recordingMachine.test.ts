import {
  IllegalTransitionError,
  RecordingState,
  begin,
  cancel,
  fail,
  finish,
  pause,
  prepare,
  resume,
  stop,
  updateElapsed,
} from "./recordingMachine"

const idle: RecordingState = { state: "Idle" }
const preparing: RecordingState = { state: "Preparing" }
const recording: RecordingState = {
  state: "Recording",
  sourceName: "Mic",
  elapsedMs: 1000,
}
const paused: RecordingState = {
  state: "Paused",
  sourceName: "Mic",
  elapsedMs: 1000,
}
const saving: RecordingState = { state: "Saving" }
const saved: RecordingState = {
  state: "Saved",
  filePath: "a.wav",
  durationMs: 1000,
  sizeBytes: 10,
}
const recoverableError: RecordingState = {
  state: "Error",
  message: "oops",
  recoverable: true,
}
const fatalError: RecordingState = {
  state: "Error",
  message: "oops",
  recoverable: false,
}

const allStates = [
  idle,
  preparing,
  recording,
  paused,
  saving,
  saved,
  recoverableError,
  fatalError,
]

function without(...exclude: RecordingState[]): RecordingState[] {
  return allStates.filter((s) => !exclude.includes(s))
}

describe("prepare", () => {
  it.each([idle, saved, recoverableError])(
    "succeeds from %o and returns Preparing",
    (from) => {
      expect(prepare(from)).toEqual({ state: "Preparing" })
    }
  )

  it.each(without(idle, saved, recoverableError))(
    "throws IllegalTransitionError from %o",
    (from) => {
      expect(() => prepare(from)).toThrow(IllegalTransitionError)
    }
  )
})

describe("begin", () => {
  it("transitions Preparing to Recording with elapsedMs 0", () => {
    expect(begin(preparing, "Mic")).toEqual({
      state: "Recording",
      sourceName: "Mic",
      elapsedMs: 0,
    })
  })

  it.each(without(preparing))("throws from %o", (from) => {
    expect(() => begin(from, "Mic")).toThrow(IllegalTransitionError)
  })
})

describe("pause", () => {
  it("transitions Recording to Paused, preserving sourceName and elapsedMs", () => {
    expect(pause(recording)).toEqual({
      state: "Paused",
      sourceName: "Mic",
      elapsedMs: 1000,
    })
  })

  it.each(without(recording))("throws from %o", (from) => {
    expect(() => pause(from)).toThrow(IllegalTransitionError)
  })
})

describe("resume", () => {
  it("transitions Paused to Recording, preserving sourceName and elapsedMs", () => {
    expect(resume(paused)).toEqual({
      state: "Recording",
      sourceName: "Mic",
      elapsedMs: 1000,
    })
  })

  it.each(without(paused))("throws from %o", (from) => {
    expect(() => resume(from)).toThrow(IllegalTransitionError)
  })
})

describe("updateElapsed", () => {
  it.each([recording, paused])("updates elapsedMs while in %o", (from) => {
    expect(updateElapsed(from, 5000)).toEqual({ ...from, elapsedMs: 5000 })
  })

  it.each(without(recording, paused))("is a no-op from %o", (from) => {
    expect(updateElapsed(from, 5000)).toEqual(from)
  })
})

describe("stop", () => {
  it.each([recording, paused])("transitions %o to Saving", (from) => {
    expect(stop(from)).toEqual({ state: "Saving" })
  })

  it.each(without(recording, paused))("throws from %o", (from) => {
    expect(() => stop(from)).toThrow(IllegalTransitionError)
  })
})

describe("finish", () => {
  const result = { filePath: "b.wav", durationMs: 2000, sizeBytes: 20 }

  it("transitions Saving to Saved with the given result", () => {
    expect(finish(saving, result)).toEqual({ state: "Saved", ...result })
  })

  it.each(without(saving))("throws from %o", (from) => {
    expect(() => finish(from, result)).toThrow(IllegalTransitionError)
  })
})

describe("cancel", () => {
  it.each([recording, paused])("transitions %o to Idle", (from) => {
    expect(cancel(from)).toEqual({ state: "Idle" })
  })

  it.each(without(recording, paused))("throws from %o", (from) => {
    expect(() => cancel(from)).toThrow(IllegalTransitionError)
  })
})

describe("fail", () => {
  it.each(allStates)("is legal from any state (%o) and returns Error", (from) => {
    expect(fail(from, "boom", true)).toEqual({
      state: "Error",
      message: "boom",
      recoverable: true,
    })
  })
})
