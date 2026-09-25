package com.soundrecorder.mobile.audiocapture

import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Test

class WavHeaderTest {
  @Test
  fun `placeholderBytes has correct RIFF WAVE fmt data layout`() {
    val bytes = WavHeader.placeholderBytes(48000)

    assertEquals(WavHeader.HEADER_SIZE, bytes.size)
    assertEquals("RIFF", String(bytes, 0, 4, Charsets.US_ASCII))
    assertEquals("WAVE", String(bytes, 8, 4, Charsets.US_ASCII))
    assertEquals("fmt ", String(bytes, 12, 4, Charsets.US_ASCII))
    assertEquals("data", String(bytes, 36, 4, Charsets.US_ASCII))

    val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.LITTLE_ENDIAN)
    assertEquals(0, buffer.getInt(4)) // RIFF chunk size placeholder
    assertEquals(16, buffer.getInt(16)) // fmt chunk size
    assertEquals(1, buffer.getShort(20).toInt()) // audio format = PCM
    assertEquals(2, buffer.getShort(22).toInt()) // channel count
    assertEquals(48000, buffer.getInt(24)) // sample rate
    assertEquals(48000 * 2 * 2, buffer.getInt(28)) // byte rate
    assertEquals(4, buffer.getShort(32).toInt()) // block align
    assertEquals(16, buffer.getShort(34).toInt()) // bits per sample
    assertEquals(0, buffer.getInt(40)) // data chunk size placeholder
  }

  @Test
  fun `patchSizes writes correct RIFF and data chunk sizes`() {
    val tempFile = File.createTempFile("wavheader-test", ".wav")
    tempFile.deleteOnExit()
    try {
      val header = WavHeader.placeholderBytes(48000)
      val dataBytes = ByteArray(1000) { it.toByte() }
      tempFile.writeBytes(header + dataBytes)

      WavHeader.patchSizes(tempFile, tempFile.length())

      val patched = tempFile.readBytes()
      val buffer = ByteBuffer.wrap(patched).order(ByteOrder.LITTLE_ENDIAN)
      assertEquals((tempFile.length() - 8).toInt(), buffer.getInt(4))
      assertEquals(1000, buffer.getInt(40))
    } finally {
      tempFile.delete()
    }
  }
}
