package com.soundrecorder.mobile.audiocapture

object StorageThreshold {
  const val MIN_FREE_BYTES: Long = 200L * 1024 * 1024 // 200 MB

  fun isBelowThreshold(freeBytes: Long): Boolean = freeBytes < MIN_FREE_BYTES
}
