package com.soundrecorder.mobile.audiocapture

import com.facebook.react.BaseReactPackage
import com.facebook.react.bridge.NativeModule
import com.facebook.react.bridge.ReactApplicationContext
import com.facebook.react.module.model.ReactModuleInfo
import com.facebook.react.module.model.ReactModuleInfoProvider

class AudioCapturePackage : BaseReactPackage() {
  override fun getModule(
    name: String,
    reactContext: ReactApplicationContext,
  ): NativeModule? {
    return if (name == AudioCaptureModule.NAME) {
      AudioCaptureModule(reactContext)
    } else {
      null
    }
  }

  override fun getReactModuleInfoProvider(): ReactModuleInfoProvider {
    return ReactModuleInfoProvider {
      mapOf(
        AudioCaptureModule.NAME to
          ReactModuleInfo(
            AudioCaptureModule.NAME,
            AudioCaptureModule.NAME,
            false,
            false,
            false,
            true,
          )
      )
    }
  }
}
