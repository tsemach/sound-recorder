import { useState } from "react"

import { invoke } from "@tauri-apps/api/core"
import { open } from "@tauri-apps/plugin-dialog"

import { Button } from "@workspace/ui/components/button"

import { errorMessage } from "../lib/errors"

export type Settings = {
  save_dir: string | null
  filename_prefix: string
  default_source_id: string | null
}

type SettingsScreenProps = {
  settings: Settings
  onSettingsChange: (next: Settings) => void
}

export function SettingsScreen({
  settings,
  onSettingsChange,
}: SettingsScreenProps) {
  const [prefixValue, setPrefixValue] = useState(settings.filename_prefix)
  const [error, setError] = useState<string | null>(null)

  async function persist(next: Settings) {
    try {
      await invoke("update_settings", { settings: next })
      onSettingsChange(next)
      setError(null)
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  async function handleChooseFolder() {
    try {
      const result = await open({ directory: true })
      if (typeof result === "string") {
        await persist({ ...settings, save_dir: result })
      }
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  async function handleSavePrefix() {
    await persist({ ...settings, filename_prefix: prefixValue })
  }

  return (
    <div className="flex flex-col gap-6">
      {error && (
        <div className="rounded border border-destructive p-2 text-sm text-destructive">
          {error}
        </div>
      )}

      <div className="flex flex-col gap-2">
        <span className="text-sm font-medium">Save location</span>
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted-foreground">
            {settings.save_dir ?? "Default location"}
          </span>
          <Button
            size="sm"
            variant="outline"
            onClick={() => void handleChooseFolder()}
          >
            Choose Folder…
          </Button>
        </div>
      </div>

      <div className="flex flex-col gap-2">
        <label className="text-sm font-medium" htmlFor="filename-prefix">
          Filename prefix
        </label>
        <div className="flex items-center gap-2">
          <input
            id="filename-prefix"
            className="rounded border p-2 text-sm"
            value={prefixValue}
            onChange={(e) => setPrefixValue(e.target.value)}
            placeholder="recording"
          />
          <Button size="sm" onClick={() => void handleSavePrefix()}>
            Save
          </Button>
        </div>
      </div>

      <p className="max-w-md text-xs text-muted-foreground">
        Recordings stay on this device. You are responsible for permission to
        record any audio you capture.
      </p>
    </div>
  )
}
