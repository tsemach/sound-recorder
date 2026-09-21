pub const SAMPLE_RATE_HZ: u64 = 48_000;

/// Duration in milliseconds represented by `sample_count` interleaved samples
/// at the given sample rate and channel count. `sample_count` counts total
/// i16 values in the buffer (all channels combined), matching what
/// `AudioCapture` frame callbacks receive.
pub fn buffer_duration_ms(sample_count: usize, sample_rate: u32, channels: u8) -> u64 {
  let frames = sample_count as u64 / channels.max(1) as u64;
  (frames * 1000) / sample_rate.max(1) as u64
}

/// Root-mean-square level of a PCM buffer, normalized to 0.0..=1.0.
pub fn compute_level(samples: &[i16]) -> f32 {
  if samples.is_empty() {
    return 0.0;
  }
  let sum_squares: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
  let mean_square = sum_squares / samples.len() as f64;
  let rms = mean_square.sqrt();
  (rms / i16::MAX as f64) as f32
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn buffer_duration_matches_sample_rate_mono() {
    // 48 mono samples at 48kHz = 1ms
    assert_eq!(buffer_duration_ms(48, 48_000, 1), 1);
    // 960 mono samples at 48kHz = 20ms
    assert_eq!(buffer_duration_ms(960, 48_000, 1), 20);
  }

  #[test]
  fn buffer_duration_accounts_for_channel_count() {
    // 1920 interleaved samples = 960 stereo frames at 48kHz = 20ms
    assert_eq!(buffer_duration_ms(1920, 48_000, 2), 20);
  }

  #[test]
  fn buffer_duration_accounts_for_sample_rate() {
    // 441 mono samples at 44.1kHz = 10ms
    assert_eq!(buffer_duration_ms(441, 44_100, 1), 10);
  }

  #[test]
  fn silence_has_zero_level() {
    let silence = vec![0_i16; 960];
    assert_eq!(compute_level(&silence), 0.0);
  }

  #[test]
  fn full_scale_square_wave_has_level_near_one() {
    let loud = vec![i16::MAX; 960];
    let level = compute_level(&loud);
    assert!(
      level > 0.99 && level <= 1.0,
      "expected near-1.0, got {level}"
    );
  }

  #[test]
  fn empty_buffer_has_zero_level() {
    assert_eq!(compute_level(&[]), 0.0);
  }
}
