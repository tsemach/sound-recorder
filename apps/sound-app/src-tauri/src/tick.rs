pub const SAMPLE_RATE_HZ: u64 = 48_000;

/// Duration in milliseconds represented by `sample_count` mono samples at `SAMPLE_RATE_HZ`.
pub fn buffer_duration_ms(sample_count: usize) -> u64 {
  (sample_count as u64 * 1000) / SAMPLE_RATE_HZ
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
  fn buffer_duration_matches_sample_rate() {
    // 48 samples at 48kHz = 1ms
    assert_eq!(buffer_duration_ms(48), 1);
    // 960 samples at 48kHz = 20ms
    assert_eq!(buffer_duration_ms(960), 20);
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
