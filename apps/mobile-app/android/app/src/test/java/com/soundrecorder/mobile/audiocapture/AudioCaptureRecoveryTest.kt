package com.soundrecorder.mobile.audiocapture

import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class AudioCaptureRecoveryTest {
  @get:Rule val tempFolder = TemporaryFolder()

  @Test
  fun `deletes an empty temp file with no audio data`() {
    val tempFile = tempFolder.newFile("recording.wav.tmp")
    tempFile.writeBytes(WavHeader.placeholderBytes(48000))

    AudioCaptureRecovery.recoverOrphans(tempFolder.root)

    assertFalse(tempFile.exists())
  }

  @Test
  fun `patches and renames a temp file that has real audio data`() {
    val tempFile = tempFolder.newFile("recording.wav.tmp")
    val header = WavHeader.placeholderBytes(48000)
    val data = ByteArray(200) { it.toByte() }
    tempFile.writeBytes(header + data)

    AudioCaptureRecovery.recoverOrphans(tempFolder.root)

    assertFalse(tempFile.exists())
    val recovered =
      tempFolder.root.listFiles { f -> f.name.startsWith("recording-") && f.name.endsWith(".wav") }
    assertEquals(1, recovered?.size)
    val buffer = ByteBuffer.wrap(recovered!![0].readBytes()).order(ByteOrder.LITTLE_ENDIAN)
    assertEquals(200, buffer.getInt(40))
  }

  @Test
  fun `ignores files that are not temp wav files`() {
    val otherFile = tempFolder.newFile("recording-123.wav")

    AudioCaptureRecovery.recoverOrphans(tempFolder.root)

    assertTrue(otherFile.exists())
  }
}
