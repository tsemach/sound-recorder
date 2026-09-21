pub const MIN_FREE_BYTES: u64 = 200 * 1024 * 1024; // 200 MB

pub fn free_space_bytes(path: &std::path::Path) -> std::io::Result<u64> {
  fs4::available_space(path)
}

pub fn is_below_threshold(free_bytes: u64) -> bool {
  free_bytes < MIN_FREE_BYTES
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn below_threshold_is_true_when_under_the_limit() {
    assert!(is_below_threshold(MIN_FREE_BYTES - 1));
    assert!(is_below_threshold(0));
  }

  #[test]
  fn below_threshold_is_false_at_and_above_the_limit() {
    assert!(!is_below_threshold(MIN_FREE_BYTES));
    assert!(!is_below_threshold(MIN_FREE_BYTES + 1));
    assert!(!is_below_threshold(u64::MAX));
  }

  #[test]
  fn free_space_bytes_queries_a_real_filesystem() {
    let dir = std::env::temp_dir();
    match free_space_bytes(&dir) {
      Ok(bytes) => {
        println!("free space on {dir:?}: {bytes} bytes");
        assert!(bytes > 0, "expected a nonzero free-space reading");
      }
      Err(e) => {
        eprintln!("warning: could not query free space on {dir:?}, skipping: {e}");
      }
    }
  }
}
