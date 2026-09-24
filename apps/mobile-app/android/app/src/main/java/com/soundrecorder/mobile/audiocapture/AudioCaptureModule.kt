package com.soundrecorder.mobile.audiocapture

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.AudioManager
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.os.Build
import androidx.core.content.ContextCompat
import com.facebook.react.bridge.ActivityEventListener
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.modules.core.DeviceEventManagerModule
import com.facebook.react.modules.core.PermissionAwareActivity
import com.facebook.react.modules.core.PermissionListener
import java.io.File

class AudioCaptureModule(private val reactContext: ReactApplicationContext) :
  NativeAudioCaptureSpec(reactContext),
  ActivityEventListener {

  companion object {
    const val NAME = "AudioCapture"
    private const val PROJECTION_REQUEST_CODE = 9001
    private const val RECORD_AUDIO_PERMISSION_REQUEST_CODE = 9002
    private const val DEFAULT_SAMPLE_RATE = 48000
    private const val TEMP_FILE_NAME = "recording.pcm.tmp"
  }

  init {
    reactContext.addActivityEventListener(this)
  }

  private var pendingStartPromise: Promise? = null
  private var mediaProjection: MediaProjection? = null
  private var engine: AudioCaptureEngine? = null

  override fun isSupported(promise: Promise) {
    promise.resolve(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
  }

  override fun listSources(promise: Promise) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
      promise.resolve(Arguments.createArray())
      return
    }
    val source = Arguments.createMap()
    source.putString("id", "system-audio")
    source.putString("name", "Device Audio")
    val sources = Arguments.createArray()
    sources.pushMap(source)
    promise.resolve(sources)
  }

  override fun startCapture(sourceId: String, promise: Promise) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
      promise.reject("UNSUPPORTED", "System audio recording requires Android 10 or later")
      return
    }
    if (pendingStartPromise != null || engine != null) {
      promise.reject("ALREADY_STARTING", "A capture is already starting or in progress")
      return
    }
    val activity = reactContext.currentActivity
    if (activity == null) {
      promise.reject("NO_ACTIVITY", "No current activity to request capture permission from")
      return
    }
    pendingStartPromise = promise
    requestRecordAudioPermission(activity)
  }

  private fun requestRecordAudioPermission(activity: Activity) {
    val permissionsNeeded = mutableListOf<String>()
    if (ContextCompat.checkSelfPermission(reactContext, Manifest.permission.RECORD_AUDIO) !=
      PackageManager.PERMISSION_GRANTED
    ) {
      permissionsNeeded.add(Manifest.permission.RECORD_AUDIO)
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      ContextCompat.checkSelfPermission(reactContext, Manifest.permission.POST_NOTIFICATIONS) !=
        PackageManager.PERMISSION_GRANTED
    ) {
      permissionsNeeded.add(Manifest.permission.POST_NOTIFICATIONS)
    }
    if (permissionsNeeded.isEmpty()) {
      requestProjection(activity)
      return
    }
    val permissionAwareActivity = activity as? PermissionAwareActivity
    if (permissionAwareActivity == null) {
      failPendingStart("NO_PERMISSION_ACTIVITY", "Activity cannot request permissions")
      return
    }
    permissionAwareActivity.requestPermissions(
      permissionsNeeded.toTypedArray(),
      RECORD_AUDIO_PERMISSION_REQUEST_CODE,
      PermissionListener { requestCode, _, grantResults ->
        if (requestCode != RECORD_AUDIO_PERMISSION_REQUEST_CODE) {
          return@PermissionListener false
        }
        if (grantResults.isNotEmpty() && grantResults.all { it == PackageManager.PERMISSION_GRANTED }) {
          requestProjection(activity)
        } else {
          failPendingStart(
            "PERMISSION_DENIED",
            "Required permissions for playback capture were denied",
          )
        }
        true
      },
    )
  }

  private fun requestProjection(activity: Activity) {
    val manager =
      reactContext.getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
    activity.startActivityForResult(manager.createScreenCaptureIntent(), PROJECTION_REQUEST_CODE)
  }

  override fun onActivityResult(
    activity: Activity,
    requestCode: Int,
    resultCode: Int,
    data: Intent?,
  ) {
    if (requestCode != PROJECTION_REQUEST_CODE) return
    if (resultCode != Activity.RESULT_OK || data == null) {
      failPendingStart("CAPTURE_DENIED", "System audio capture permission was denied")
      return
    }
    val manager =
      reactContext.getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
    val projection = manager.getMediaProjection(resultCode, data)
    if (projection == null) {
      failPendingStart("CAPTURE_FAILED", "Could not obtain media projection")
      return
    }
    mediaProjection = projection
    try {
      AudioCaptureService.start(reactContext)

      val audioManager = reactContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
      val sampleRate =
        audioManager.getProperty(AudioManager.PROPERTY_OUTPUT_SAMPLE_RATE)?.toIntOrNull()
          ?: DEFAULT_SAMPLE_RATE

      val outputFile = File(reactContext.filesDir, TEMP_FILE_NAME)
      val captureEngine =
        AudioCaptureEngine(
          mediaProjection = projection,
          outputFile = outputFile,
          onLevel = { level -> emitLevel(level) },
        )
      engine = captureEngine
      captureEngine.start(sampleRate)

      pendingStartPromise?.resolve(null)
      pendingStartPromise = null
    } catch (e: Exception) {
      engine?.stop()
      engine = null
      mediaProjection?.stop()
      mediaProjection = null
      AudioCaptureService.stop(reactContext)
      failPendingStart("CAPTURE_START_FAILED", e.message ?: "Failed to start audio capture")
    }
  }

  override fun onNewIntent(intent: Intent) {}

  private fun failPendingStart(code: String, message: String) {
    pendingStartPromise?.reject(code, message)
    pendingStartPromise = null
  }

  private fun emitLevel(level: Float) {
    reactContext
      .getJSModule(DeviceEventManagerModule.RCTDeviceEventEmitter::class.java)
      .emit("AudioCaptureLevel", level.toDouble())
  }

  override fun pauseCapture() {
    engine?.pause()
  }

  override fun resumeCapture() {
    engine?.resume()
  }

  override fun stopCapture(promise: Promise) {
    val captureEngine = engine
    if (captureEngine == null) {
      promise.reject("NOT_RECORDING", "No active capture to stop")
      return
    }
    val sizeBytes = captureEngine.stop()
    engine = null
    mediaProjection?.stop()
    mediaProjection = null
    AudioCaptureService.stop(reactContext)

    val tempFile = File(reactContext.filesDir, TEMP_FILE_NAME)
    val finalFile = File(reactContext.filesDir, "recording-${System.currentTimeMillis()}.pcm")
    if (!tempFile.renameTo(finalFile)) {
      promise.reject("RENAME_FAILED", "Could not finalize the recording file")
      return
    }

    val result = Arguments.createMap()
    result.putString("filePath", finalFile.absolutePath)
    result.putDouble("sizeBytes", sizeBytes.toDouble())
    promise.resolve(result)
  }

  override fun addListener(eventName: String) {}

  override fun removeListeners(count: Double) {}
}
