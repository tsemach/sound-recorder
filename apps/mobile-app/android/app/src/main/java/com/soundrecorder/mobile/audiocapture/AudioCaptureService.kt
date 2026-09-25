package com.soundrecorder.mobile.audiocapture

import android.app.Activity
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.annotation.RequiresApi

@RequiresApi(Build.VERSION_CODES.O)
class AudioCaptureService : Service() {
  // Confirmed on-device (Android 10 / MIUI): MediaProjectionManager.getMediaProjection()
  // throws SecurityException("Media projections require a foreground service of type
  // ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION") unless a matching foreground
  // service is ALREADY running when it's called. Starting the service and then calling
  // getMediaProjection() back in AudioCaptureModule doesn't satisfy this: startForegroundService()
  // only *requests* a service start — onStartCommand() (and this class's startForeground()
  // call within it) runs asynchronously, dispatched later on the main thread, with no
  // guarantee it has run before the caller's next line executes. The only ordering Android
  // actually guarantees is within a single onStartCommand() call, so this service now owns
  // obtaining the MediaProjection and reports the result back to the module via a callback,
  // instead of the module obtaining it itself right after requesting the service start.
  companion object {
    private const val CHANNEL_ID = "audio_capture"
    private const val NOTIFICATION_ID = 1001
    private const val EXTRA_RESULT_CODE = "resultCode"
    private const val EXTRA_RESULT_DATA = "resultData"

    interface Callback {
      fun onForegroundReady(resultCode: Int, data: Intent)
    }

    var callback: Callback? = null

    fun start(context: Context, resultCode: Int, data: Intent) {
      val intent = Intent(context, AudioCaptureService::class.java)
      intent.putExtra(EXTRA_RESULT_CODE, resultCode)
      intent.putExtra(EXTRA_RESULT_DATA, data)
      context.startForegroundService(intent)
    }

    fun stop(context: Context) {
      callback = null
      context.stopService(Intent(context, AudioCaptureService::class.java))
    }
  }

  override fun onBind(intent: Intent?): IBinder? = null

  override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
    createNotificationChannel()
    val notification =
      Notification.Builder(this, CHANNEL_ID)
        .setContentTitle("Sound Recorder")
        .setContentText("Recording system audio")
        .setSmallIcon(android.R.drawable.ic_btn_speak_now)
        .build()

    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
      startForeground(
        NOTIFICATION_ID,
        notification,
        ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION,
      )
    } else {
      startForeground(NOTIFICATION_ID, notification)
    }

    // Only now (after startForeground() has actually run) is it safe to obtain the
    // MediaProjection — this is the entire reason this step lives here and not in the
    // module right after requesting the service start.
    val resultCode = intent?.getIntExtra(EXTRA_RESULT_CODE, Activity.RESULT_CANCELED)
      ?: Activity.RESULT_CANCELED
    @Suppress("DEPRECATION") val data = intent?.getParcelableExtra<Intent>(EXTRA_RESULT_DATA)
    if (resultCode == Activity.RESULT_OK && data != null) {
      callback?.onForegroundReady(resultCode, data)
    }

    return START_NOT_STICKY
  }

  private fun createNotificationChannel() {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    val manager = getSystemService(NotificationManager::class.java)
    val channel =
      NotificationChannel(CHANNEL_ID, "Audio Capture", NotificationManager.IMPORTANCE_LOW)
    manager.createNotificationChannel(channel)
  }
}
