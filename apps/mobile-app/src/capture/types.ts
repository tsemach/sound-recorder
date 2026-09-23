export type AudioSource = { id: string; name: string }

export interface AudioCapture {
  listSources(): Promise<AudioSource[]>
  start(sourceId: string, onFrame: (frame: Int16Array) => void): Promise<void>
  pause(): void
  resume(): void
  stop(): Promise<void>
}
