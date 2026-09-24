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

export default TurboModuleRegistry.get<Spec>("AudioCapture")
