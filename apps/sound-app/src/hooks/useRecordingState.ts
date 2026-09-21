import { useCallback, useEffect, useState } from "react"

import { invoke } from "@tauri-apps/api/core"
import { listen } from "@tauri-apps/api/event"

import { errorMessage } from "../lib/errors"

export type AudioSource = { id: string; name: string }

export type RecordingState =
  | { state: "Idle" }
  | { state: "Preparing" }
  | { state: "Recording"; source_name: string; elapsed_ms: number }
  | { state: "Paused"; source_name: string; elapsed_ms: number }
  | { state: "Saving" }
  | {
      state: "Saved"
      file_path: string
      duration_ms: number
      size_bytes: number
    }
  | { state: "Error"; message: string; recoverable: boolean }

type Tick = { elapsed_ms: number; level: number }

export function useRecordingState() {
  const [state, setState] = useState<RecordingState>({ state: "Idle" })
  const [elapsedMs, setElapsedMs] = useState(0)
  const [level, setLevel] = useState(0)
  const [sources, setSources] = useState<AudioSource[]>([])
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    let unlistenState: (() => void) | undefined
    let unlistenTick: (() => void) | undefined

    listen<RecordingState>("recording-state-changed", (event) => {
      setState(event.payload)
      if (
        event.payload.state === "Recording" ||
        event.payload.state === "Paused"
      ) {
        setElapsedMs(event.payload.elapsed_ms)
      }
      if (event.payload.state !== "Recording") {
        setLevel(0)
      }
      if (event.payload.state === "Error") {
        setError(event.payload.message)
      }
      if (event.payload.state === "Recording") {
        setError(null)
      }
    })
      .then((fn) => {
        if (cancelled) {
          fn()
        } else {
          unlistenState = fn
        }
      })
      .catch((err) => setError(errorMessage(err)))

    listen<Tick>("recording-tick", (event) => {
      setElapsedMs(event.payload.elapsed_ms)
      setLevel(event.payload.level)
    })
      .then((fn) => {
        if (cancelled) {
          fn()
        } else {
          unlistenTick = fn
        }
      })
      .catch((err) => setError(errorMessage(err)))

    invoke<AudioSource[]>("list_sources")
      .then(setSources)
      .catch((err) => setError(errorMessage(err)))

    return () => {
      cancelled = true
      unlistenState?.()
      unlistenTick?.()
    }
  }, [])

  const startRecording = useCallback(async (sourceId: string) => {
    setError(null)
    try {
      await invoke("start_recording", { sourceId })
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const pauseRecording = useCallback(async () => {
    setError(null)
    try {
      await invoke("pause_recording")
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const resumeRecording = useCallback(async () => {
    setError(null)
    try {
      await invoke("resume_recording")
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const stopRecording = useCallback(async () => {
    setError(null)
    try {
      await invoke("stop_recording")
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

  const cancelRecording = useCallback(async () => {
    setError(null)
    try {
      await invoke("cancel_recording")
    } catch (err) {
      setError(errorMessage(err))
    }
  }, [])

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
