import { NativeEventEmitter } from "react-native"

import NativeAudioCapture from "../specs/NativeAudioCapture"
import type { Spec } from "../specs/NativeAudioCapture"
import type { AudioCapture, AudioSource, CaptureResult } from "./types"

const LEVEL_EVENT = "AudioCaptureLevel"

type AudioCaptureEvents = {
  [LEVEL_EVENT]: [number]
}

export class AndroidPlaybackCapture implements AudioCapture {
  private nativeModule: Spec
  private emitter: NativeEventEmitter<AudioCaptureEvents>
  private subscription: { remove: () => void } | null = null

  constructor() {
    if (NativeAudioCapture == null) {
      throw new Error(
        "AudioCapture native module is not available on this platform"
      )
    }
    this.nativeModule = NativeAudioCapture
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
    onLevel: (level: number) => void
  ): Promise<void> {
    this.subscription?.remove()
    this.subscription = this.emitter.addListener(
      LEVEL_EVENT,
      (level: number) => {
        onLevel(level)
      }
    )
    try {
      await this.nativeModule.startCapture(sourceId)
    } catch (err) {
      this.subscription?.remove()
      this.subscription = null
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
    this.subscription?.remove()
    this.subscription = null
    return this.nativeModule.stopCapture()
  }
}
