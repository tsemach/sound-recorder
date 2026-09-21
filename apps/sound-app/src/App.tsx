import { useState } from "react"

import { Button } from "@workspace/ui/components/button"

import { RecordingsList } from "./components/RecordingsList"
import { ThemeProvider } from "./components/theme-provider"
import { useRecordingState } from "./hooks/useRecordingState"
import { formatDuration } from "./lib/format"

export function App() {
  const {
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
  } = useRecordingState()

  const [sourceOverride, setSourceOverride] = useState<string | null>(null)
  const selectedSourceId = sourceOverride ?? sources[0]?.id ?? ""
  const [view, setView] = useState<"recorder" | "recordings">("recorder")

  const canStart =
    state.state === "Idle" ||
    state.state === "Saved" ||
    (state.state === "Error" && state.recoverable)
  const isRecording = state.state === "Recording"
  const isPaused = state.state === "Paused"
  const isActive = isRecording || isPaused

  function handleCancel() {
    if (window.confirm("Discard this recording?")) {
      void cancelRecording()
    }
  }

  return (
    <ThemeProvider>
      <div className="flex min-h-svh flex-col gap-4 p-6">
        <div className="flex items-center justify-between">
          <h1 className="font-medium">Sound Recorder</h1>
          <div className="flex items-center gap-3">
            {isActive && (
              <span className="flex items-center gap-1.5 text-sm text-muted-foreground">
                <span className="h-2 w-2 rounded-full bg-destructive" />
                Recording &middot; {formatDuration(elapsedMs)}
              </span>
            )}
            <Button
              variant="outline"
              size="sm"
              onClick={() =>
                setView(view === "recorder" ? "recordings" : "recorder")
              }
            >
              {view === "recorder" ? "Recordings" : "Back to Recorder"}
            </Button>
          </div>
        </div>

        {view === "recordings" ? (
          <RecordingsList />
        ) : (
          <>
            {error && (
              <div className="rounded border border-destructive p-2 text-sm text-destructive">
                {error}
              </div>
            )}

            {canStart && sources.length > 0 && (
              <select
                className="w-fit rounded border p-2 text-sm"
                value={selectedSourceId}
                onChange={(e) => setSourceOverride(e.target.value)}
              >
                {sources.map((source) => (
                  <option key={source.id} value={source.id}>
                    {source.name}
                  </option>
                ))}
              </select>
            )}

            <div className="font-mono text-2xl">
              {formatDuration(isActive ? elapsedMs : 0)}
            </div>

            {isActive && (
              <div className="h-2 w-full max-w-xs rounded bg-muted">
                <div
                  className="h-2 rounded bg-primary transition-all"
                  style={{ width: `${Math.min(level, 1) * 100}%` }}
                />
              </div>
            )}

            {state.state === "Saving" && (
              <div className="text-sm text-muted-foreground">Saving…</div>
            )}

            <div className="flex gap-2">
              {canStart && sources.length > 0 && (
                <Button onClick={() => void startRecording(selectedSourceId)}>
                  Record
                </Button>
              )}
              {isRecording && (
                <Button onClick={() => void pauseRecording()}>Pause</Button>
              )}
              {isPaused && (
                <Button onClick={() => void resumeRecording()}>Resume</Button>
              )}
              {isActive && (
                <Button onClick={() => void stopRecording()}>Stop</Button>
              )}
              {isActive && <Button onClick={handleCancel}>Cancel</Button>}
            </div>
          </>
        )}
      </div>
    </ThemeProvider>
  )
}
