package com.soundrecorder.mobile.audiocapture

import org.junit.Assert.assertEquals
import org.junit.Test

class AudioCaptureEngineTest {
  @Test
  fun `computeLevel returns zero for silence`() {
    val buffer = ShortArray(4) { 0 }
    assertEquals(0.0f, AudioCaptureEngine.computeLevel(buffer, 4), 0.0001f)
  }

  @Test
  fun `computeLevel returns close to one for full-scale samples`() {
    val buffer = shortArrayOf(32767, -32768, 32767, -32768)
    assertEquals(1.0f, AudioCaptureEngine.computeLevel(buffer, 4), 0.01f)
  }

  @Test
  fun `computeLevel only considers readCount samples`() {
    val buffer = shortArrayOf(32767, 32767, 0, 0)
    assertEquals(1.0f, AudioCaptureEngine.computeLevel(buffer, 2), 0.01f)
  }
}
