package com.soundrecorder.mobile.audiocapture

import java.io.File

// Recovers *.wav.tmp files left behind by a crash, OS-initiated kill, or an
// interrupted discard — same shape as sound-app's own orphan-recovery, run
// once at app startup rather than as a live in-process error handler.
object AudioCaptureRecovery {
  private const val TEMP_SUFFIX = ".wav.tmp"

  fun recoverOrphans(directory: File) {
    val tempFiles = directory.listFiles { file -> file.name.endsWith(TEMP_SUFFIX) } ?: return
    for (tempFile in tempFiles) {
      recoverOne(tempFile)
    }
  }

  private fun recoverOne(tempFile: File) {
    val size = tempFile.length()
    if (size <= WavHeader.HEADER_SIZE) {
      tempFile.delete()
      return
    }
    WavHeader.patchSizes(tempFile, size)
    val finalFile = File(tempFile.parentFile, "recording-${tempFile.lastModified()}.wav")
    tempFile.renameTo(finalFile)
  }
}
