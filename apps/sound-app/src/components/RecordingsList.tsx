import { useEffect, useState } from "react"

import { convertFileSrc, invoke } from "@tauri-apps/api/core"
import { revealItemInDir } from "@tauri-apps/plugin-opener"

import { Button } from "@workspace/ui/components/button"

import { errorMessage } from "../lib/errors"
import { formatDuration } from "../lib/format"

export type RecordingMeta = {
  path: string
  filename: string
  created_at_ms: number
  duration_ms: number
  size_bytes: number
  format: string
}

function formatSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(0)} KB`
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function formatDate(ms: number): string {
  return new Date(ms).toLocaleString()
}

export function RecordingsList() {
  const [recordings, setRecordings] = useState<RecordingMeta[]>([])
  const [error, setError] = useState<string | null>(null)
  const [renamingPath, setRenamingPath] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState("")
  const [audioUrls, setAudioUrls] = useState<Record<string, string>>({})

  async function refresh() {
    try {
      const result = await invoke<RecordingMeta[]>("list_recordings")
      setRecordings(result)
      setError(null)
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  useEffect(() => {
    void refresh()
  }, [])

  // WebKitGTK on Linux can't stream playback directly from the asset
  // protocol (GStreamer doesn't reliably handle its range requests), so
  // each recording's bytes are fetched into a Blob and played from an
  // object URL instead -- this works on every platform.
  useEffect(() => {
    let cancelled = false
    const urls: Record<string, string> = {}

    async function loadAll() {
      await Promise.all(
        recordings.map(async (recording) => {
          try {
            const response = await fetch(convertFileSrc(recording.path))
            const bytes = await response.arrayBuffer()
            const blob = new Blob([bytes], { type: "audio/wav" })
            urls[recording.path] = URL.createObjectURL(blob)
          } catch {
            // Leave this recording without a playable URL; its <audio>
            // element just renders with no source rather than blocking
            // the rest of the list.
          }
        })
      )
      if (cancelled) {
        Object.values(urls).forEach((url) => URL.revokeObjectURL(url))
        return
      }
      setAudioUrls(urls)
    }

    void loadAll()

    return () => {
      cancelled = true
      Object.values(urls).forEach((url) => URL.revokeObjectURL(url))
    }
  }, [recordings])

  function startRename(recording: RecordingMeta) {
    setRenamingPath(recording.path)
    setRenameValue(recording.filename)
  }

  async function confirmRename(recording: RecordingMeta) {
    try {
      await invoke("rename_recording", {
        oldName: recording.filename,
        newName: renameValue,
      })
      setRenamingPath(null)
      await refresh()
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  async function handleDelete(recording: RecordingMeta) {
    if (!window.confirm(`Delete "${recording.filename}"?`)) {
      return
    }
    try {
      await invoke("delete_recording", { name: recording.filename })
      await refresh()
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  async function handleReveal(recording: RecordingMeta) {
    try {
      await revealItemInDir(recording.path)
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  return (
    <div className="flex flex-col gap-4">
      {error && (
        <div className="rounded border border-destructive p-2 text-sm text-destructive">
          {error}
        </div>
      )}

      {recordings.length === 0 && (
        <p className="text-sm text-muted-foreground">No recordings yet.</p>
      )}

      <ul className="flex flex-col gap-3">
        {recordings.map((recording) => (
          <li
            key={recording.path}
            className="flex flex-col gap-2 rounded border p-3"
          >
            <div className="flex items-center justify-between gap-2">
              {renamingPath === recording.path ? (
                <input
                  className="w-full rounded border p-1 text-sm"
                  value={renameValue}
                  onChange={(e) => setRenameValue(e.target.value)}
                  aria-label={`New name for ${recording.filename}`}
                  autoFocus
                />
              ) : (
                <span className="font-medium">{recording.filename}</span>
              )}
              <span className="text-xs text-muted-foreground">
                {formatDate(recording.created_at_ms)}
              </span>
            </div>

            <audio
              controls
              src={audioUrls[recording.path]}
              aria-label={`Play ${recording.filename}`}
              className="w-full"
            />

            <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
              <span>
                {recording.format} &middot;{" "}
                {formatDuration(recording.duration_ms)} &middot;{" "}
                {formatSize(recording.size_bytes)}
              </span>
              <div className="flex gap-2">
                {renamingPath === recording.path ? (
                  <>
                    <Button
                      size="sm"
                      onClick={() => void confirmRename(recording)}
                    >
                      Save
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setRenamingPath(null)}
                    >
                      Cancel
                    </Button>
                  </>
                ) : (
                  <>
                    <Button
                      size="sm"
                      onClick={() => startRename(recording)}
                      aria-label={`Rename ${recording.filename}`}
                    >
                      Rename
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => void handleReveal(recording)}
                      aria-label={`Reveal ${recording.filename}`}
                    >
                      Reveal
                    </Button>
                    <Button
                      size="sm"
                      variant="destructive"
                      onClick={() => void handleDelete(recording)}
                      aria-label={`Delete ${recording.filename}`}
                    >
                      Delete
                    </Button>
                  </>
                )}
              </div>
            </div>
          </li>
        ))}
      </ul>
    </div>
  )
}
