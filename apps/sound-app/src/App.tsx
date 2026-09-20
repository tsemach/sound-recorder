import { useState } from "react"

import { invoke } from "@tauri-apps/api/core"
import { Button } from "@workspace/ui/components/button"

import { ThemeProvider } from "./components/theme-provider"

export function App() {
  const [pingResult, setPingResult] = useState<string | null>(null)

  async function handlePing() {
    const result = await invoke<string>("ping")
    setPingResult(result)
  }

  return (
    <ThemeProvider>
      <div className="flex min-h-svh p-6">
        <div className="flex max-w-md min-w-0 flex-col gap-4 text-sm leading-loose">
          <div>
            <h1 className="font-medium">Project ready!</h1>
            <p>You may now add components and start building.</p>
            <p>We&apos;ve already added the button component for you.</p>
            <Button className="mt-2">Button</Button>
          </div>
          <div>
            <Button onClick={handlePing}>Ping backend</Button>
            {pingResult !== null && <p className="mt-2">{pingResult}</p>}
          </div>
          <div className="text-muted-foreground font-mono text-xs">
            (Press <kbd>d</kbd> to toggle dark mode)
          </div>
        </div>
      </div>
    </ThemeProvider>
  )
}
