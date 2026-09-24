import { NativeModules } from "react-native"

// Plain legacy native module lookup, not a codegen'd TurboModule spec.
// RN 0.87.1's New Architecture Java-Spec TurboModule interop was confirmed
// via on-device testing to construct AudioCaptureModule successfully on the
// native side, but never exposed it to JS (TurboModuleRegistry.get always
// returned null despite the native constructor log firing). The legacy
// bridge path — still a first-class, fully-supported mechanism in
// Bridgeless RN, since most third-party libraries haven't migrated to
// TurboModules either — works correctly and sidesteps that interop layer
// entirely. See AudioCaptureModule.kt for the full investigation notes.
export interface Spec {
  isSupported(): Promise<boolean>
  listSources(): Promise<Array<{ id: string; name: string }>>
  startCapture(sourceId: string): Promise<void>
  pauseCapture(): void
  resumeCapture(): void
  stopCapture(): Promise<{ filePath: string; sizeBytes: number }>
  addListener(eventName: string): void
  removeListeners(count: number): void
}

export function getAudioCaptureNativeModule(): Spec | null {
  return (NativeModules.AudioCapture as Spec | undefined) ?? null
}
