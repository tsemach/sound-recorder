use std::path::{Path, PathBuf};

/// Recomputes and overwrites a WAV file's two size fields (RIFF chunk size
/// at byte offset 4, data chunk size at byte offset 40) from its actual
/// byte count on disk. This is a fallback for a temp file that crashed
/// before its first `hound` `flush()` call ever completed (the placeholder
/// header `hound::WavWriter::create` writes has size fields of 0, which
/// don't match however many raw sample bytes actually made it to disk).
/// Only correct for the plain 44-byte PCM16, <=2-channel header `hound`
/// writes for this app's recordings (never the 64-byte WAVEFORMATEXTENSIBLE
/// header, which `hound` only uses above 2 channels or 16 bits).
fn patch_header(path: &Path) -> std::io::Result<()> {
  let total_size = std::fs::metadata(path)?.len();
  if total_size < 44 {
    return Err(std::io::Error::new(
      std::io::ErrorKind::InvalidData,
      "smaller than a WAV header",
    ));
  }
  // hound uses a larger (64-byte) WAVEFORMATEXTENSIBLE header for >2
  // channels or >16-bit recordings, where the data-size field is NOT at
  // byte offset 40 -- writing there would corrupt the fmt chunk instead of
  // patching a size field. This app's recordings are always <=2 channels
  // at 16-bit (see AudioFormat), so a file this large before any flush
  // patched its header is not something this fallback can safely handle;
  // let it fall through to deletion in recover_one instead of corrupting it.
  if total_size > 10_000_000 {
    return Err(std::io::Error::new(
      std::io::ErrorKind::InvalidData,
      "unexpectedly large unflushed file, refusing to guess its header layout",
    ));
  }
  let data_len = (total_size - 44) as u32;
  let riff_len = (total_size - 8) as u32;

  use std::io::{Seek, SeekFrom, Write};
  let mut f = std::fs::OpenOptions::new().write(true).open(path)?;
  f.seek(SeekFrom::Start(4))?;
  f.write_all(&riff_len.to_le_bytes())?;
  f.seek(SeekFrom::Start(40))?;
  f.write_all(&data_len.to_le_bytes())?;
  f.flush()?;
  Ok(())
}

/// Recovers one orphaned `*.wav.tmp` file: promotes it to a final, playable
/// `.wav` (same name, `.tmp` suffix stripped) if it has any audio data,
/// deletes it otherwise. Returns the final path if promoted, `None` if
/// deleted.
fn recover_one(tmp_path: &Path) -> std::io::Result<Option<PathBuf>> {
  let already_valid = hound::WavReader::open(tmp_path)
    .map(|r| r.len() > 0)
    .unwrap_or(false);

  if !already_valid {
    // Try the manual patch fallback (a crash before the first flush).
    let _ = patch_header(tmp_path);
  }

  let has_data = hound::WavReader::open(tmp_path)
    .map(|r| r.len() > 0)
    .unwrap_or(false);

  if !has_data {
    std::fs::remove_file(tmp_path)?;
    return Ok(None);
  }

  let final_path = tmp_path.with_extension(""); // strips the trailing ".tmp"
  std::fs::rename(tmp_path, &final_path)?;
  Ok(Some(final_path))
}

/// Scans `dir` for `*.wav.tmp` files left behind by an interrupted recording
/// (a real capture/write error, a force-quit, or a crash) and recovers each
/// one. Meant to run once, synchronously, at app startup before the window
/// opens — fast, since it's at most a directory listing plus a couple of
/// small stray files in the overwhelmingly common case.
pub fn recover_orphaned_recordings(dir: &Path) -> std::io::Result<Vec<PathBuf>> {
  let mut recovered = Vec::new();
  if !dir.exists() {
    return Ok(recovered);
  }
  for entry in std::fs::read_dir(dir)? {
    let entry = entry?;
    let path = entry.path();
    let is_temp_wav = path.extension().and_then(|e| e.to_str()) == Some("tmp")
      && path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.ends_with(".wav"))
        .unwrap_or(false);
    if is_temp_wav {
      match recover_one(&path) {
        Ok(Some(final_path)) => recovered.push(final_path),
        Ok(None) => {}
        Err(e) => {
          log::warn!("Could not recover orphaned recording {path:?}: {e}");
        }
      }
    }
  }
  Ok(recovered)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_flushed_temp(dir: &Path, name: &str, samples: &[i16]) -> PathBuf {
    let path = dir.join(name);
    let spec = hound::WavSpec {
      channels: 2,
      sample_rate: 48_000,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(&path, spec).unwrap();
    for &s in samples {
      writer.write_sample(s).unwrap();
    }
    writer.flush().unwrap();
    std::mem::forget(writer); // simulate a crash right after the last flush
    path
  }

  fn make_unflushed_spill_temp(dir: &Path, name: &str, samples: &[i16]) -> PathBuf {
    let path = dir.join(name);
    let spec = hound::WavSpec {
      channels: 2,
      sample_rate: 48_000,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let file = std::fs::File::create(&path).unwrap();
    let small_buf = std::io::BufWriter::with_capacity(64, file); // forces a spill quickly
    let mut writer = hound::WavWriter::new(small_buf, spec).unwrap();
    for &s in samples {
      writer.write_sample(s).unwrap();
    }
    std::mem::forget(writer); // crash before any explicit flush
    path
  }

  #[test]
  fn recovers_a_cleanly_flushed_orphan_by_just_renaming() {
    let dir = std::env::temp_dir().join("pr5_recovery_test_flushed");
    std::fs::create_dir_all(&dir).unwrap();
    let tmp = make_flushed_temp(&dir, "recording-a.wav.tmp", &[1, 2, 3, 4]);

    let recovered = recover_orphaned_recordings(&dir).unwrap();
    assert_eq!(recovered.len(), 1);
    assert!(recovered[0].to_string_lossy().ends_with("recording-a.wav"));
    assert!(!std::fs::exists(&tmp).unwrap());

    let reader = hound::WavReader::open(&recovered[0]).unwrap();
    let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
    assert_eq!(samples, vec![1, 2, 3, 4]);
    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn recovers_a_never_flushed_spill_orphan_via_manual_patch() {
    let dir = std::env::temp_dir().join("pr5_recovery_test_spill");
    std::fs::create_dir_all(&dir).unwrap();
    let big_signal: Vec<i16> = (0..2000).collect();
    let tmp = make_unflushed_spill_temp(&dir, "recording-b.wav.tmp", &big_signal);

    let recovered = recover_orphaned_recordings(&dir).unwrap();
    assert_eq!(recovered.len(), 1);
    assert!(!std::fs::exists(&tmp).unwrap());

    let reader = hound::WavReader::open(&recovered[0]).unwrap();
    assert!(reader.len() > 0);
    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn deletes_a_temp_file_with_no_audio_data() {
    let dir = std::env::temp_dir().join("pr5_recovery_test_empty");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("recording-c.wav.tmp");
    let spec = hound::WavSpec {
      channels: 2,
      sample_rate: 48_000,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let writer = hound::WavWriter::create(&path, spec).unwrap();
    writer.finalize().unwrap(); // zero samples, but a valid header

    let recovered = recover_orphaned_recordings(&dir).unwrap();
    assert_eq!(recovered.len(), 0);
    assert!(!std::fs::exists(&path).unwrap());
    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn ignores_non_tmp_files_and_missing_directories() {
    let dir = std::env::temp_dir().join("pr5_recovery_test_missing");
    let _ = std::fs::remove_dir_all(&dir);
    let recovered = recover_orphaned_recordings(&dir).unwrap();
    assert!(recovered.is_empty());
  }
}
