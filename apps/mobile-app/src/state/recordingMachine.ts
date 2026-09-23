export type RecordingState =
  | { state: "Idle" }
  | { state: "Preparing" }
  | { state: "Recording"; sourceName: string; elapsedMs: number }
  | { state: "Paused"; sourceName: string; elapsedMs: number }
  | { state: "Saving" }
  | { state: "Saved"; filePath: string; durationMs: number; sizeBytes: number }
  | { state: "Error"; message: string; recoverable: boolean }

export type SavedResult = {
  filePath: string
  durationMs: number
  sizeBytes: number
}

export class IllegalTransitionError extends Error {
  constructor(message: string) {
    super(message)
    this.name = "IllegalTransitionError"
  }
}

function canStart(state: RecordingState): boolean {
  return (
    state.state === "Idle" ||
    state.state === "Saved" ||
    (state.state === "Error" && state.recoverable)
  )
}

export function prepare(state: RecordingState): RecordingState {
  if (!canStart(state)) {
    throw new IllegalTransitionError(`Cannot prepare from state ${state.state}`)
  }
  return { state: "Preparing" }
}

export function begin(
  state: RecordingState,
  sourceName: string
): RecordingState {
  if (state.state !== "Preparing") {
    throw new IllegalTransitionError(`Cannot begin from state ${state.state}`)
  }
  return { state: "Recording", sourceName, elapsedMs: 0 }
}

export function pause(state: RecordingState): RecordingState {
  if (state.state !== "Recording") {
    throw new IllegalTransitionError(`Cannot pause from state ${state.state}`)
  }
  return {
    state: "Paused",
    sourceName: state.sourceName,
    elapsedMs: state.elapsedMs,
  }
}

export function resume(state: RecordingState): RecordingState {
  if (state.state !== "Paused") {
    throw new IllegalTransitionError(`Cannot resume from state ${state.state}`)
  }
  return {
    state: "Recording",
    sourceName: state.sourceName,
    elapsedMs: state.elapsedMs,
  }
}

export function updateElapsed(
  state: RecordingState,
  elapsedMs: number
): RecordingState {
  if (state.state === "Recording" || state.state === "Paused") {
    return { ...state, elapsedMs }
  }
  return state
}

export function stop(state: RecordingState): RecordingState {
  if (state.state !== "Recording" && state.state !== "Paused") {
    throw new IllegalTransitionError(`Cannot stop from state ${state.state}`)
  }
  return { state: "Saving" }
}

export function finish(
  state: RecordingState,
  result: SavedResult
): RecordingState {
  if (state.state !== "Saving") {
    throw new IllegalTransitionError(`Cannot finish from state ${state.state}`)
  }
  return { state: "Saved", ...result }
}

export function cancel(state: RecordingState): RecordingState {
  if (state.state !== "Recording" && state.state !== "Paused") {
    throw new IllegalTransitionError(`Cannot cancel from state ${state.state}`)
  }
  return { state: "Idle" }
}

export function fail(
  state: RecordingState,
  message: string,
  recoverable: boolean
): RecordingState {
  return { state: "Error", message, recoverable }
}
