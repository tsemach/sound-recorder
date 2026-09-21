export type CommandError = { message: string; recoverable: boolean }

export function errorMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as CommandError).message)
  }
  return String(err)
}
