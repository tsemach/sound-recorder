export type AudioSource = { id: string; name: string }

export type CaptureResult = { filePath: string; sizeBytes: number }

export interface AudioCapture {
  listSources(): Promise<AudioSource[]>
  start(sourceId: string, onLevel: (level: number) => void): Promise<void>
  pause(): void
  resume(): void
  stop(): Promise<CaptureResult>
  discard(): Promise<void>
}
