package com.soundrecorder.mobile.audiocapture

import android.util.Log
import java.io.File

// Recovers *.wav.tmp files left behind by a crash, OS-initiated kill, or an
// interrupted discard — same shape as sound-app's own orphan-recovery. Runs
// once, at first construction of the native module (lazily, on first JS
// access — not literally at app process startup), rather than as a live
// in-process error handler.
object AudioCaptureRecovery {
  private const val TAG = "AudioCaptureRecovery"
  private const val TEMP_SUFFIX = ".wav.tmp"

  fun recoverOrphans(directory: File) {
    val tempFiles = directory.listFiles { file -> file.name.endsWith(TEMP_SUFFIX) } ?: return
    for (tempFile in tempFiles) {
      try {
        recoverOne(tempFile)
      } catch (e: Exception) {
        Log.w(TAG, "failed to recover $tempFile", e)
      }
    }
  }

  private fun recoverOne(tempFile: File) {
    val size = tempFile.length()
    if (size <= WavHeader.HEADER_SIZE) {
      if (!tempFile.delete()) {
        Log.w(TAG, "failed to delete/rename $tempFile")
      }
      return
    }
    WavHeader.patchSizes(tempFile, size)
    val finalFile = File(tempFile.parentFile, "recording-${tempFile.lastModified()}.wav")
    if (finalFile.exists()) {
      Log.w(TAG, "recovery target $finalFile already exists, skipping rename of $tempFile")
      return
    }
    if (!tempFile.renameTo(finalFile)) {
      Log.w(TAG, "failed to delete/rename $tempFile")
    }
  }
}
