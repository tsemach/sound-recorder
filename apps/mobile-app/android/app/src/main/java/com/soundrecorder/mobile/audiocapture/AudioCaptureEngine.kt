package com.soundrecorder.mobile.audiocapture

import android.Manifest
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioPlaybackCaptureConfiguration
import android.media.AudioRecord
import android.media.projection.MediaProjection
import android.os.Build
import androidx.annotation.RequiresApi
import androidx.annotation.RequiresPermission
import java.io.File
import java.io.FileOutputStream
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.sqrt

@RequiresApi(Build.VERSION_CODES.Q)
class AudioCaptureEngine(
  private val mediaProjection: MediaProjection,
  private val outputFile: File,
  private val onLevel: (Float) -> Unit,
) {
  companion object {
    private const val LEVEL_EMIT_INTERVAL_MS = 100L

    fun computeLevel(buffer: ShortArray, readCount: Int): Float {
      if (readCount == 0) return 0f
      var sumSquares = 0.0
      for (i in 0 until readCount) {
        val normalized = buffer[i] / 32768.0
        sumSquares += normalized * normalized
      }
      return sqrt(sumSquares / readCount).toFloat()
    }
  }

  private var audioRecord: AudioRecord? = null
  private var thread: Thread? = null
  private var outputStream: FileOutputStream? = null
  private val running = AtomicBoolean(false)
  private val paused = AtomicBoolean(false)

  @RequiresPermission(Manifest.permission.RECORD_AUDIO)
  fun start(sampleRate: Int) {
    val captureConfig =
      AudioPlaybackCaptureConfiguration.Builder(mediaProjection)
        .addMatchingUsage(AudioAttributes.USAGE_MEDIA)
        .addMatchingUsage(AudioAttributes.USAGE_GAME)
        .addMatchingUsage(AudioAttributes.USAGE_UNKNOWN)
        .build()

    val channelMask = AudioFormat.CHANNEL_IN_STEREO
    val encoding = AudioFormat.ENCODING_PCM_16BIT
    val minBufferSize = AudioRecord.getMinBufferSize(sampleRate, channelMask, encoding)
    val bufferSizeInBytes = if (minBufferSize > 0) minBufferSize * 2 else sampleRate * 2

    val audioFormat =
      AudioFormat.Builder()
        .setEncoding(encoding)
        .setSampleRate(sampleRate)
        .setChannelMask(channelMask)
        .build()

    val record =
      AudioRecord.Builder()
        .setAudioFormat(audioFormat)
        .setBufferSizeInBytes(bufferSizeInBytes)
        .setAudioPlaybackCaptureConfig(captureConfig)
        .build()

    audioRecord = record
    outputStream = FileOutputStream(outputFile)
    running.set(true)
    paused.set(false)
    record.startRecording()

    val readThread =
      Thread {
        val buffer = ShortArray(bufferSizeInBytes / 2)
        var lastEmitAt = 0L
        while (running.get()) {
          try {
            val readCount = record.read(buffer, 0, buffer.size)
            if (readCount > 0 && !paused.get()) {
              writeSamples(buffer, readCount)

              val now = System.currentTimeMillis()
              if (now - lastEmitAt >= LEVEL_EMIT_INTERVAL_MS) {
                onLevel(computeLevel(buffer, readCount))
                lastEmitAt = now
              }
            }
          } catch (e: Exception) {
            android.util.Log.e("AudioCaptureEngine", "capture read loop failed", e)
            running.set(false)
          }
        }
      }
    thread = readThread
    readThread.start()
  }

  private fun writeSamples(buffer: ShortArray, readCount: Int) {
    val byteBuffer = ByteBuffer.allocate(readCount * 2).order(ByteOrder.LITTLE_ENDIAN)
    for (i in 0 until readCount) {
      byteBuffer.putShort(buffer[i])
    }
    outputStream?.write(byteBuffer.array())
  }

  fun pause() {
    paused.set(true)
  }

  fun resume() {
    paused.set(false)
  }

  fun stop(): Long {
    running.set(false)
    audioRecord?.stop()
    thread?.join(2000)
    thread = null
    audioRecord?.release()
    audioRecord = null
    outputStream?.flush()
    outputStream?.close()
    outputStream = null
    return outputFile.length()
  }
}
