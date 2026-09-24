import type { TurboModule } from "react-native"
import { TurboModuleRegistry } from "react-native"

export interface Spec extends TurboModule {
  isSupported(): Promise<boolean>
  listSources(): Promise<Array<{ id: string; name: string }>>
  startCapture(sourceId: string): Promise<void>
  pauseCapture(): void
  resumeCapture(): void
  stopCapture(): Promise<{ filePath: string; sizeBytes: number }>
  addListener(eventName: string): void
  removeListeners(count: number): void
}

// Deliberately NOT resolved at module scope (e.g. `TurboModuleRegistry.get(...)`
// evaluated as a top-level export). In React Native's Bridgeless architecture,
// JS module evaluation can happen before the native TurboModule registry has
// finished registering packages on cold start, which would permanently bake in
// a `null` result for any code that only reads a top-level singleton. Calling
// this function lazily, at the point a caller actually needs the module (well
// after the JS bundle has loaded and rendering has begun), avoids that race.
export function getAudioCaptureNativeModule(): Spec | null {
  return TurboModuleRegistry.get<Spec>("AudioCapture") ?? null
}
