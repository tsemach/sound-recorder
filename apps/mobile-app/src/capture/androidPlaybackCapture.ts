import { NativeEventEmitter } from "react-native"

import NativeAudioCapture from "../specs/NativeAudioCapture"
import type { AudioCapture, AudioSource, CaptureResult } from "./types"

const LEVEL_EVENT = "AudioCaptureLevel"

type AudioCaptureEvents = {
  [LEVEL_EVENT]: [number]
}

export class AndroidPlaybackCapture implements AudioCapture {
  private emitter = new NativeEventEmitter<AudioCaptureEvents>(
    NativeAudioCapture as unknown as ConstructorParameters<
      typeof NativeEventEmitter
    >[0]
  )
  private subscription: { remove: () => void } | null = null

  async listSources(): Promise<AudioSource[]> {
    const supported = await NativeAudioCapture.isSupported()
    if (!supported) return []
    return NativeAudioCapture.listSources()
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
      await NativeAudioCapture.startCapture(sourceId)
    } catch (err) {
      this.subscription?.remove()
      this.subscription = null
      throw err
    }
  }

  pause(): void {
    NativeAudioCapture.pauseCapture()
  }

  resume(): void {
    NativeAudioCapture.resumeCapture()
  }

  async stop(): Promise<CaptureResult> {
    this.subscription?.remove()
    this.subscription = null
    return NativeAudioCapture.stopCapture()
  }
}
