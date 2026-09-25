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
import androidx.annotation.RequiresApi
import androidx.annotation.RequiresPermission
import androidx.core.content.ContextCompat
import com.facebook.react.bridge.ActivityEventListener
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.bridge.ReactContextBaseJavaModule
import com.facebook.react.bridge.ReactMethod
import com.facebook.react.modules.core.DeviceEventManagerModule
import com.facebook.react.modules.core.PermissionAwareActivity
import com.facebook.react.modules.core.PermissionListener
import java.io.File

// Registered as a legacy (non-TurboModule) native module, not a codegen'd
// TurboModule spec. RN 0.87.1's New Architecture Java-Spec TurboModule
// interop (DefaultTurboModuleManagerDelegate's javaModuleProvider ->
// TurboModuleRegistry.get()) was confirmed via on-device testing + logging
// to construct this module successfully on the native side, but never
// exposed it to JS (TurboModuleRegistry.get("AudioCapture") returned null
// every time despite the native constructor log firing). Traced the failure
// through ReactModuleInfo, DefaultReactHost, ReactPackageTurboModuleManagerDelegate,
// ReactInstance, and DefaultTurboModuleManagerDelegate's C++ implementation
// without finding a definitive root cause short of native (JNI) debugging
// tools not available in this environment. The legacy bridge path (still a
// first-class, fully-supported mechanism in Bridgeless RN, since most
// third-party libraries haven't migrated to TurboModules either) works
// correctly and sidesteps that interop layer entirely.
class AudioCaptureModule(private val reactContext: ReactApplicationContext) :
  ReactContextBaseJavaModule(reactContext),
  ActivityEventListener {

  companion object {
    const val NAME = "AudioCapture"
    private const val PROJECTION_REQUEST_CODE = 9001
    private const val RECORD_AUDIO_PERMISSION_REQUEST_CODE = 9002
    private const val DEFAULT_SAMPLE_RATE = 48000
    private const val TEMP_FILE_NAME = "recording.wav.tmp"
  }

  override fun getName(): String = NAME

  init {
    reactContext.addActivityEventListener(this)
    AudioCaptureRecovery.recoverOrphans(reactContext.filesDir)
  }

  private var pendingStartPromise: Promise? = null
  private var mediaProjection: MediaProjection? = null
  private var engine: AudioCaptureEngine? = null

  @ReactMethod
  fun isSupported(promise: Promise) {
    promise.resolve(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
  }

  @ReactMethod
  fun listSources(promise: Promise) {
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

  @ReactMethod
  fun startCapture(sourceId: String, promise: Promise) {
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

  // Reached only via the projection-request flow kicked off from
  // startCapture(), which already rejected on SDK_INT < Q and only requests
  // the projection intent after RECORD_AUDIO was confirmed granted; lint
  // can't trace either guarantee across the Activity result callback, so
  // both are made explicit here.
  @RequiresApi(Build.VERSION_CODES.Q)
  @RequiresPermission(Manifest.permission.RECORD_AUDIO)
  override fun onActivityResult(
    activity: Activity,
    requestCode: Int,
    resultCode: Int,
    data: Intent?,
  ) {
    if (requestCode != PROJECTION_REQUEST_CODE) return
    if (pendingStartPromise == null) {
      return
    }
    if (resultCode != Activity.RESULT_OK || data == null) {
      failPendingStart("CAPTURE_DENIED", "System audio capture permission was denied")
      return
    }
    // The service must actually call startForeground() before
    // MediaProjectionManager.getMediaProjection() is safe to call (confirmed on-device:
    // it throws SecurityException otherwise) — so the service owns obtaining the
    // projection, in onStartCommand(), right after startForeground(), and reports back
    // here via this callback rather than us calling getMediaProjection() ourselves.
    AudioCaptureService.callback =
      object : AudioCaptureService.Companion.Callback {
        @RequiresApi(Build.VERSION_CODES.Q)
        @RequiresPermission(Manifest.permission.RECORD_AUDIO)
        override fun onForegroundReady(resultCode: Int, data: Intent) {
          startEngineNowThatForegroundIsConfirmed(resultCode, data)
        }
      }
    AudioCaptureService.start(reactContext, resultCode, data)
  }

  // Only ever invoked from AudioCaptureService's callback, itself only reachable
  // from onActivityResult()'s guarded flow (SDK_INT >= Q, RECORD_AUDIO granted) —
  // lint can't trace either guarantee through the service/callback indirection.
  @RequiresApi(Build.VERSION_CODES.Q)
  @RequiresPermission(Manifest.permission.RECORD_AUDIO)
  private fun startEngineNowThatForegroundIsConfirmed(resultCode: Int, data: Intent) {
    val manager =
      reactContext.getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager
    val projection = manager.getMediaProjection(resultCode, data)
    if (projection == null) {
      failPendingStart("CAPTURE_FAILED", "Could not obtain media projection")
      AudioCaptureService.stop(reactContext)
      return
    }
    mediaProjection = projection
    try {
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
      engine?.discardAndDelete()
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

  // A non-null engine only ever exists after onActivityResult constructed it
  // under SDK_INT >= Q; lint can't see that invariant across fields.
  @ReactMethod
  @RequiresApi(Build.VERSION_CODES.Q)
  fun pauseCapture() {
    engine?.pause()
  }

  @ReactMethod
  @RequiresApi(Build.VERSION_CODES.Q)
  fun resumeCapture() {
    engine?.resume()
  }

  @ReactMethod
  @RequiresApi(Build.VERSION_CODES.Q)
  fun stopCapture(promise: Promise) {
    val captureEngine = engine
    if (captureEngine == null) {
      promise.reject("NOT_RECORDING", "No active capture to stop")
      return
    }
    val sizeBytes: Long
    try {
      sizeBytes = captureEngine.stop()
    } catch (e: Exception) {
      engine = null
      mediaProjection?.stop()
      mediaProjection = null
      AudioCaptureService.stop(reactContext)
      promise.reject("STOP_FAILED", e.message ?: "Failed to stop audio capture")
      return
    }
    engine = null
    mediaProjection?.stop()
    mediaProjection = null
    AudioCaptureService.stop(reactContext)

    val tempFile = File(reactContext.filesDir, TEMP_FILE_NAME)
    val finalFile = File(reactContext.filesDir, "recording-${System.currentTimeMillis()}.wav")
    if (!tempFile.renameTo(finalFile)) {
      promise.reject("RENAME_FAILED", "Could not finalize the recording file")
      return
    }

    val result = Arguments.createMap()
    result.putString("filePath", finalFile.absolutePath)
    result.putDouble("sizeBytes", sizeBytes.toDouble())
    promise.resolve(result)
  }

  @ReactMethod
  @RequiresApi(Build.VERSION_CODES.Q)
  fun discardCapture(promise: Promise) {
    val captureEngine = engine
    if (captureEngine == null) {
      promise.reject("NOT_RECORDING", "No active capture to discard")
      return
    }
    try {
      captureEngine.discardAndDelete()
    } catch (e: Exception) {
      engine = null
      mediaProjection?.stop()
      mediaProjection = null
      AudioCaptureService.stop(reactContext)
      promise.reject("DISCARD_FAILED", e.message ?: "Failed to discard audio capture")
      return
    }
    engine = null
    mediaProjection?.stop()
    mediaProjection = null
    AudioCaptureService.stop(reactContext)
    promise.resolve(null)
  }

  @ReactMethod
  fun addListener(eventName: String) {}

  @ReactMethod
  fun removeListeners(count: Double) {}
}
