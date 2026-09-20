import { StrictMode } from "react"
import { createRoot } from "react-dom/client"

import "@workspace/ui/globals.css"

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <div>Sound Recorder</div>
  </StrictMode>
)
