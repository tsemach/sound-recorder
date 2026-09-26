import { NativeEventEmitter } from "react-native"

import { getAudioCaptureNativeModule } from "../specs/NativeAudioCapture"
import type { Spec } from "../specs/NativeAudioCapture"
import type { AudioCapture, AudioSource, CaptureResult } from "./types"

const LEVEL_EVENT = "AudioCaptureLevel"
const ERROR_EVENT = "AudioCaptureError"

type AudioCaptureEvents = {
  [LEVEL_EVENT]: [number]
  [ERROR_EVENT]: [string]
}

export class AndroidPlaybackCapture implements AudioCapture {
  private nativeModule: Spec
  private emitter: NativeEventEmitter<AudioCaptureEvents>
  private subscriptions: { remove: () => void }[] = []

  constructor() {
    const nativeModule = getAudioCaptureNativeModule()
    if (nativeModule == null) {
      throw new Error(
        "AudioCapture native module is not available on this platform"
      )
    }
    this.nativeModule = nativeModule
    this.emitter = new NativeEventEmitter<AudioCaptureEvents>(
      this.nativeModule as unknown as ConstructorParameters<
        typeof NativeEventEmitter
      >[0]
    )
  }

  async listSources(): Promise<AudioSource[]> {
    const supported = await this.nativeModule.isSupported()
    if (!supported) return []
    return this.nativeModule.listSources()
  }

  async start(
    sourceId: string,
    onLevel: (level: number) => void,
    onError?: (message: string) => void
  ): Promise<void> {
    this.removeSubscriptions()
    this.subscriptions.push(
      this.emitter.addListener(LEVEL_EVENT, (level: number) => {
        onLevel(level)
      })
    )
    this.subscriptions.push(
      this.emitter.addListener(ERROR_EVENT, (message: string) => {
        onError?.(message)
      })
    )
    try {
      await this.nativeModule.startCapture(sourceId)
    } catch (err) {
      this.removeSubscriptions()
      throw err
    }
  }

  pause(): void {
    this.nativeModule.pauseCapture()
  }

  resume(): void {
    this.nativeModule.resumeCapture()
  }

  async stop(): Promise<CaptureResult> {
    this.removeSubscriptions()
    return this.nativeModule.stopCapture()
  }

  async discard(): Promise<void> {
    this.removeSubscriptions()
    await this.nativeModule.discardCapture()
  }

  private removeSubscriptions(): void {
    for (const subscription of this.subscriptions) {
      subscription.remove()
    }
    this.subscriptions = []
  }
}
