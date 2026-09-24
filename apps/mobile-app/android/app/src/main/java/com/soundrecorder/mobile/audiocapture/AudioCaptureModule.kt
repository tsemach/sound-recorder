package com.soundrecorder.mobile.audiocapture

import android.os.Build
import com.facebook.react.bridge.Arguments
import com.facebook.react.bridge.Promise
import com.facebook.react.bridge.ReactApplicationContext

class AudioCaptureModule(reactContext: ReactApplicationContext) :
  NativeAudioCaptureSpec(reactContext) {

  companion object {
    const val NAME = "AudioCapture"
  }

  override fun isSupported(promise: Promise) {
    promise.resolve(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
  }

  override fun listSources(promise: Promise) {
    val source = Arguments.createMap()
    source.putString("id", "system-audio")
    source.putString("name", "Device Audio")
    val sources = Arguments.createArray()
    sources.pushMap(source)
    promise.resolve(sources)
  }

  override fun startCapture(sourceId: String, promise: Promise) {
    promise.resolve(null)
  }

  override fun pauseCapture() {}

  override fun resumeCapture() {}

  override fun stopCapture(promise: Promise) {
    val result = Arguments.createMap()
    result.putString("filePath", "")
    result.putDouble("sizeBytes", 0.0)
    promise.resolve(result)
  }

  override fun addListener(eventName: String) {}

  override fun removeListeners(count: Double) {}
}
