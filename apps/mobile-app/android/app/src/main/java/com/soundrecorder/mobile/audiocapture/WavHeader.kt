package com.soundrecorder.mobile.audiocapture

import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.ByteOrder

object WavHeader {
  const val HEADER_SIZE = 44
  private const val BITS_PER_SAMPLE = 16
  private const val CHANNEL_COUNT = 2

  fun placeholderBytes(sampleRate: Int): ByteArray {
    val byteRate = sampleRate * CHANNEL_COUNT * BITS_PER_SAMPLE / 8
    val blockAlign = CHANNEL_COUNT * BITS_PER_SAMPLE / 8
    val buffer = ByteBuffer.allocate(HEADER_SIZE).order(ByteOrder.LITTLE_ENDIAN)
    buffer.put("RIFF".toByteArray(Charsets.US_ASCII))
    buffer.putInt(0) // RIFF chunk size — patched at finalize
    buffer.put("WAVE".toByteArray(Charsets.US_ASCII))
    buffer.put("fmt ".toByteArray(Charsets.US_ASCII))
    buffer.putInt(16) // fmt chunk size (PCM)
    buffer.putShort(1) // audio format = PCM
    buffer.putShort(CHANNEL_COUNT.toShort())
    buffer.putInt(sampleRate)
    buffer.putInt(byteRate)
    buffer.putShort(blockAlign.toShort())
    buffer.putShort(BITS_PER_SAMPLE.toShort())
    buffer.put("data".toByteArray(Charsets.US_ASCII))
    buffer.putInt(0) // data chunk size — patched at finalize
    return buffer.array()
  }

  /** [totalSize] is the full file size in bytes, including the 44-byte header. */
  fun patchSizes(file: File, totalSize: Long) {
    val dataLength = totalSize - HEADER_SIZE
    RandomAccessFile(file, "rw").use { raf ->
      raf.seek(4)
      raf.write(leBytes((totalSize - 8).toInt()))
      raf.seek(40)
      raf.write(leBytes(dataLength.toInt()))
    }
  }

  private fun leBytes(value: Int): ByteArray =
    ByteBuffer.allocate(4).order(ByteOrder.LITTLE_ENDIAN).putInt(value).array()
}
