import type { AudioCapture, AudioSource } from "./types"

const SAMPLE_RATE = 48000
const BUFFER_MS = 20
const FREQUENCY_HZ = 440

export class FakeCapture implements AudioCapture {
  private intervalId: ReturnType<typeof setInterval> | null = null
  private paused = false
  private phase = 0

  async listSources(): Promise<AudioSource[]> {
    return [
      { id: "fake-system-audio", name: "Fake System Audio" },
      { id: "fake-microphone", name: "Fake Microphone" },
    ]
  }

  async start(
    _sourceId: string,
    onFrame: (frame: Int16Array) => void
  ): Promise<void> {
    if (this.intervalId !== null) {
      throw new Error("FakeCapture.start() called while already running")
    }
    this.paused = false
    this.phase = 0
    const samplesPerBuffer = Math.floor((SAMPLE_RATE * BUFFER_MS) / 1000)

    this.intervalId = setInterval(() => {
      if (this.paused) return
      const buffer = new Int16Array(samplesPerBuffer)
      for (let i = 0; i < samplesPerBuffer; i++) {
        buffer[i] = Math.round(Math.sin(this.phase) * 0.2 * 32767)
        this.phase += (2 * Math.PI * FREQUENCY_HZ) / SAMPLE_RATE
      }
      onFrame(buffer)
    }, BUFFER_MS)
  }

  pause(): void {
    this.paused = true
  }

  resume(): void {
    this.paused = false
  }

  async stop(): Promise<void> {
    if (this.intervalId !== null) {
      clearInterval(this.intervalId)
      this.intervalId = null
    }
  }
}
