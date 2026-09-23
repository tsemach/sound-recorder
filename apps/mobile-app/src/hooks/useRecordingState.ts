import { useCallback, useEffect, useRef, useState } from "react"

import type { AudioCapture, AudioSource } from "../capture/types"
import { FakeCapture } from "../capture/fakeCapture"
import { errorMessage } from "../lib/errorMessage"
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
} from "../state/recordingMachine"

const TICK_MS = 100

function computeLevel(frame: Int16Array): number {
  if (frame.length === 0) return 0
  let sumSquares = 0
  for (let i = 0; i < frame.length; i++) {
    const normalized = frame[i] / 32768
    sumSquares += normalized * normalized
  }
  return Math.sqrt(sumSquares / frame.length)
}

export function useRecordingState(capture: AudioCapture = new FakeCapture()) {
  const [state, setState] = useState<RecordingState>({ state: "Idle" })
  const [level, setLevel] = useState(0)
  const [sources, setSources] = useState<AudioSource[]>([])
  const [error, setError] = useState<string | null>(null)

  const stateRef = useRef<RecordingState>(state)
  const startedAtRef = useRef(0)
  const pausedAccumRef = useRef(0)
  const pausedAtRef = useRef(0)
  const latestFrameRef = useRef<Int16Array | null>(null)
  const tickIntervalRef = useRef<ReturnType<typeof setInterval> | null>(null)

  const applyState = useCallback((next: RecordingState) => {
    stateRef.current = next
    setState(next)
  }, [])

  const handleFailure = useCallback(
    (err: unknown) => {
      if (err instanceof IllegalTransitionError) {
        setError(errorMessage(err))
        return
      }
      applyState(fail(stateRef.current, errorMessage(err), true))
      setError(errorMessage(err))
    },
    [applyState]
  )

  useEffect(() => {
    let cancelled = false
    capture
      .listSources()
      .then((result) => {
        if (!cancelled) setSources(result)
      })
      .catch((err) => {
        if (!cancelled) setError(errorMessage(err))
      })
    return () => {
      cancelled = true
    }
  }, [capture])

  const stopTickLoop = useCallback(() => {
    if (tickIntervalRef.current !== null) {
      clearInterval(tickIntervalRef.current)
      tickIntervalRef.current = null
    }
  }, [])

  const startTickLoop = useCallback(() => {
    stopTickLoop()
    tickIntervalRef.current = setInterval(() => {
      const elapsedMs = Date.now() - startedAtRef.current - pausedAccumRef.current
      applyState(updateElapsed(stateRef.current, elapsedMs))
      setLevel(latestFrameRef.current ? computeLevel(latestFrameRef.current) : 0)
    }, TICK_MS)
  }, [applyState, stopTickLoop])

  useEffect(() => stopTickLoop, [stopTickLoop])

  const startRecording = useCallback(
    async (sourceId: string) => {
      setError(null)
      try {
        const source = sources.find((candidate) => candidate.id === sourceId)
        const sourceName = source?.name ?? sourceId
        applyState(prepare(stateRef.current))
        latestFrameRef.current = null
        await capture.start(sourceId, (frame) => {
          latestFrameRef.current = frame
        })
        startedAtRef.current = Date.now()
        pausedAccumRef.current = 0
        applyState(begin(stateRef.current, sourceName))
        startTickLoop()
      } catch (err) {
        stopTickLoop()
        handleFailure(err)
      }
    },
    [applyState, capture, handleFailure, sources, startTickLoop, stopTickLoop]
  )

  const pauseRecording = useCallback(() => {
    setError(null)
    try {
      applyState(pause(stateRef.current))
      capture.pause()
      pausedAtRef.current = Date.now()
      stopTickLoop()
    } catch (err) {
      handleFailure(err)
    }
  }, [applyState, capture, handleFailure, stopTickLoop])

  const resumeRecording = useCallback(() => {
    setError(null)
    try {
      applyState(resume(stateRef.current))
      capture.resume()
      pausedAccumRef.current += Date.now() - pausedAtRef.current
      startTickLoop()
    } catch (err) {
      handleFailure(err)
    }
  }, [applyState, capture, handleFailure, startTickLoop])

  const stopRecording = useCallback(async () => {
    setError(null)
    try {
      applyState(stop(stateRef.current))
      stopTickLoop()
      await capture.stop()
      const durationMs = Date.now() - startedAtRef.current - pausedAccumRef.current
      applyState(
        finish(stateRef.current, {
          filePath: `fake/recording-${Date.now()}.wav`,
          durationMs,
          sizeBytes: 0,
        })
      )
    } catch (err) {
      handleFailure(err)
    }
  }, [applyState, capture, handleFailure, stopTickLoop])

  const cancelRecording = useCallback(async () => {
    setError(null)
    try {
      const next = cancel(stateRef.current)
      stopTickLoop()
      await capture.stop()
      applyState(next)
    } catch (err) {
      handleFailure(err)
    }
  }, [applyState, capture, handleFailure, stopTickLoop])

  const elapsedMs =
    state.state === "Recording" || state.state === "Paused" ? state.elapsedMs : 0

  return {
    state,
    elapsedMs,
    level,
    sources,
    error,
    startRecording,
    pauseRecording,
    resumeRecording,
    stopRecording,
    cancelRecording,
  }
}
