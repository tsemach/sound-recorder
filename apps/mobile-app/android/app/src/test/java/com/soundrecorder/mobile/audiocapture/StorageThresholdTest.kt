package com.soundrecorder.mobile.audiocapture

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StorageThresholdTest {
  @Test
  fun `below threshold returns true when free bytes is less than the minimum`() {
    assertTrue(StorageThreshold.isBelowThreshold(StorageThreshold.MIN_FREE_BYTES - 1))
  }

  @Test
  fun `below threshold returns false when free bytes equals the minimum`() {
    assertFalse(StorageThreshold.isBelowThreshold(StorageThreshold.MIN_FREE_BYTES))
  }

  @Test
  fun `below threshold returns false when free bytes is greater than the minimum`() {
    assertFalse(StorageThreshold.isBelowThreshold(StorageThreshold.MIN_FREE_BYTES + 1))
  }
}
