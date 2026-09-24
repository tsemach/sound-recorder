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

export function useRecordingState(capture?: AudioCapture) {
  const fallbackRef = useRef<AudioCapture | null>(null)
  if (fallbackRef.current === null) {
    fallbackRef.current = new FakeCapture()
  }
  const activeCapture = capture ?? fallbackRef.current

  const [state, setState] = useState<RecordingState>({ state: "Idle" })
  const [level, setLevel] = useState(0)
  const [sources, setSources] = useState<AudioSource[]>([])
  const [error, setError] = useState<string | null>(null)

  const stateRef = useRef<RecordingState>(state)
  const startedAtRef = useRef(0)
  const pausedAccumRef = useRef(0)
  const pausedAtRef = useRef(0)
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
    activeCapture
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
  }, [activeCapture])

  const stopTickLoop = useCallback(() => {
    if (tickIntervalRef.current !== null) {
      clearInterval(tickIntervalRef.current)
      tickIntervalRef.current = null
    }
  }, [])

  const startTickLoop = useCallback(() => {
    stopTickLoop()
    tickIntervalRef.current = setInterval(() => {
      const elapsedMs =
        Date.now() - startedAtRef.current - pausedAccumRef.current
      applyState(updateElapsed(stateRef.current, elapsedMs))
    }, TICK_MS)
  }, [applyState, stopTickLoop])

  useEffect(() => {
    return () => {
      stopTickLoop()
      void activeCapture.stop()
    }
    // Empty deps: this must run only on actual unmount, not whenever
    // activeCapture/stopTickLoop identity changes. activeCapture is stable
    // across renders when no capture prop is passed (see the lazy fallback
    // ref above).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  const startRecording = useCallback(
    async (sourceId: string) => {
      setError(null)
      try {
        const source = sources.find((candidate) => candidate.id === sourceId)
        const sourceName = source?.name ?? sourceId
        applyState(prepare(stateRef.current))
        await activeCapture.start(sourceId, (level) => {
          setLevel(level)
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
    [
      activeCapture,
      applyState,
      handleFailure,
      sources,
      startTickLoop,
      stopTickLoop,
    ]
  )

  const pauseRecording = useCallback(() => {
    setError(null)
    try {
      applyState(pause(stateRef.current))
      activeCapture.pause()
      pausedAtRef.current = Date.now()
      stopTickLoop()
      setLevel(0)
    } catch (err) {
      handleFailure(err)
    }
  }, [activeCapture, applyState, handleFailure, stopTickLoop])

  const resumeRecording = useCallback(() => {
    setError(null)
    try {
      applyState(resume(stateRef.current))
      activeCapture.resume()
      pausedAccumRef.current += Date.now() - pausedAtRef.current
      startTickLoop()
    } catch (err) {
      handleFailure(err)
    }
  }, [activeCapture, applyState, handleFailure, startTickLoop])

  const stopRecording = useCallback(async () => {
    setError(null)
    try {
      if (stateRef.current.state === "Paused") {
        pausedAccumRef.current += Date.now() - pausedAtRef.current
      }
      applyState(stop(stateRef.current))
      stopTickLoop()
      const { filePath, sizeBytes } = await activeCapture.stop()
      const durationMs =
        Date.now() - startedAtRef.current - pausedAccumRef.current
      applyState(finish(stateRef.current, { filePath, durationMs, sizeBytes }))
    } catch (err) {
      handleFailure(err)
    }
  }, [activeCapture, applyState, handleFailure, stopTickLoop])

  const cancelRecording = useCallback(async () => {
    setError(null)
    try {
      applyState(cancel(stateRef.current))
      stopTickLoop()
      await activeCapture.stop()
    } catch (err) {
      handleFailure(err)
    }
  }, [activeCapture, applyState, handleFailure, stopTickLoop])

  const elapsedMs =
    state.state === "Recording" || state.state === "Paused"
      ? state.elapsedMs
      : 0

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
