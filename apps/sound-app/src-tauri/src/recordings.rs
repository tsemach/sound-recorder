use std::path::Path;

#[derive(serde::Serialize)]
pub struct RecordingMeta {
  pub path: String,
  pub filename: String,
  pub created_at_ms: u64,
  pub duration_ms: u64,
  pub size_bytes: u64,
  pub format: String,
}

fn is_temp_wav(path: &Path) -> bool {
  path.extension().and_then(|e| e.to_str()) == Some("tmp")
    && path
      .file_stem()
      .and_then(|s| s.to_str())
      .map(|s| s.ends_with(".wav"))
      .unwrap_or(false)
}

fn is_real_wav(path: &Path) -> bool {
  path.extension().and_then(|e| e.to_str()) == Some("wav") && !is_temp_wav(path)
}

fn read_one(path: &Path) -> Option<RecordingMeta> {
  let metadata = std::fs::metadata(path).ok()?;
  let created_at_ms = metadata
    .modified()
    .ok()?
    .duration_since(std::time::UNIX_EPOCH)
    .ok()?
    .as_millis() as u64;
  let size_bytes = metadata.len();

  let reader = hound::WavReader::open(path).ok()?;
  let spec = reader.spec();
  let duration_samples = reader.duration();
  let duration_ms = (duration_samples as u64 * 1000) / spec.sample_rate.max(1) as u64;
  let format = format!(
    "WAV {} kHz · {} ch · {}-bit",
    spec.sample_rate / 1000,
    spec.channels,
    spec.bits_per_sample
  );

  Some(RecordingMeta {
    path: path.to_string_lossy().to_string(),
    filename: path.file_name()?.to_string_lossy().to_string(),
    created_at_ms,
    duration_ms,
    size_bytes,
    format,
  })
}

pub fn list_recordings(dir: &Path) -> std::io::Result<Vec<RecordingMeta>> {
  let mut recordings = Vec::new();
  if !dir.exists() {
    return Ok(recordings);
  }
  for entry in std::fs::read_dir(dir)? {
    let entry = entry?;
    let path = entry.path();
    if is_real_wav(&path) {
      if let Some(meta) = read_one(&path) {
        recordings.push(meta);
      }
    }
  }
  recordings.sort_by(|a, b| {
    b.created_at_ms
      .cmp(&a.created_at_ms)
      .then(a.filename.cmp(&b.filename))
  });
  Ok(recordings)
}

/// Rejects a `name` containing a path separator (defense-in-depth against a
/// malformed path escaping the save directory).
fn validate_name(name: &str) -> Result<(), String> {
  if name.contains('/') || name.contains('\\') {
    return Err("Name cannot contain a path separator".to_string());
  }
  Ok(())
}

/// Rejects an `old_name` or `new_name` containing a path separator, a
/// `new_name` not ending in `.wav`, or a `new_name` colliding with an
/// existing file. On success, renames `dir/old_name` to `dir/new_name` and
/// returns the new full path.
pub fn rename_recording(dir: &Path, old_name: &str, new_name: &str) -> Result<String, String> {
  validate_name(old_name)?;
  validate_name(new_name)?;
  if !new_name.ends_with(".wav") {
    return Err("New name must end in .wav".to_string());
  }
  let old_path = dir.join(old_name);
  let new_path = dir.join(new_name);
  if new_path != old_path && new_path.exists() {
    return Err("A recording with that name already exists".to_string());
  }
  std::fs::rename(&old_path, &new_path).map_err(|e| format!("Could not rename: {e}"))?;
  Ok(new_path.to_string_lossy().to_string())
}

/// Rejects a `name` containing a path separator (defense-in-depth against a
/// malformed path escaping the save directory), then deletes `dir/name`.
pub fn delete_recording(dir: &Path, name: &str) -> Result<(), String> {
  validate_name(name)?;
  std::fs::remove_file(dir.join(name)).map_err(|e| format!("Could not delete: {e}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn write_test_wav(path: &Path, samples: &[i16]) {
    let spec = hound::WavSpec {
      channels: 2,
      sample_rate: 48_000,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for &s in samples {
      writer.write_sample(s).unwrap();
    }
    writer.finalize().unwrap();
  }

  #[test]
  fn list_recordings_reads_duration_from_the_wav_header() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_duration");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("recording-a.wav");
    // 960 interleaved samples / 2 channels = 480 frames; 480/48000*1000 = 10ms
    let samples: Vec<i16> = (0..960).collect();
    write_test_wav(&path, &samples);

    let recordings = list_recordings(&dir).unwrap();
    assert_eq!(recordings.len(), 1);
    assert_eq!(recordings[0].duration_ms, 10);
    assert_eq!(recordings[0].size_bytes, 44 + 960 * 2);
    assert_eq!(recordings[0].filename, "recording-a.wav");
    assert_eq!(recordings[0].format, "WAV 48 kHz · 2 ch · 16-bit");

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn list_recordings_excludes_wav_tmp_files() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_tmp_filter");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("recording-a.wav"), &[1, 2, 3, 4]);
    write_test_wav(&dir.join("recording-b.wav.tmp"), &[1, 2, 3, 4]);

    let recordings = list_recordings(&dir).unwrap();
    assert_eq!(recordings.len(), 1);
    assert_eq!(recordings[0].filename, "recording-a.wav");

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn list_recordings_recognizes_any_wav_file_not_just_the_recording_prefix() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_any_name");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("Interview with Alex.wav"), &[1, 2, 3, 4]);

    let recordings = list_recordings(&dir).unwrap();
    assert_eq!(recordings.len(), 1);
    assert_eq!(recordings[0].filename, "Interview with Alex.wav");

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn list_recordings_sorts_newest_first() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_sort");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("first.wav"), &[1, 2]);
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_test_wav(&dir.join("second.wav"), &[1, 2]);

    let recordings = list_recordings(&dir).unwrap();
    assert_eq!(recordings.len(), 2);
    assert_eq!(recordings[0].filename, "second.wav");
    assert_eq!(recordings[1].filename, "first.wav");

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn list_recordings_skips_a_corrupt_file_instead_of_aborting_the_scan() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_corrupt");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("good.wav"), &[1, 2, 3, 4]);
    std::fs::write(dir.join("corrupt.wav"), b"not a real wav file").unwrap();

    let recordings = list_recordings(&dir).unwrap();
    assert_eq!(recordings.len(), 1);
    assert_eq!(recordings[0].filename, "good.wav");

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn rename_recording_succeeds_and_renames_the_file() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_rename_ok");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("old.wav"), &[1, 2]);

    let result = rename_recording(&dir, "old.wav", "new.wav");
    assert!(result.is_ok());
    assert!(!dir.join("old.wav").exists());
    assert!(dir.join("new.wav").exists());

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn rename_recording_to_the_same_name_is_a_no_op_not_a_collision() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_rename_same_name");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("same.wav"), &[1, 2]);

    let result = rename_recording(&dir, "same.wav", "same.wav");
    assert!(result.is_ok());
    assert!(dir.join("same.wav").exists());

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn rename_recording_rejects_a_path_separator() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_rename_sep");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("old.wav"), &[1, 2]);

    let result = rename_recording(&dir, "old.wav", "../escape.wav");
    assert!(result.is_err());
    assert!(dir.join("old.wav").exists());

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn rename_recording_rejects_a_path_separator_in_old_name() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_rename_old_name_sep");
    std::fs::create_dir_all(&dir).unwrap();

    let result = rename_recording(&dir, "../escape.wav", "new.wav");
    assert!(result.is_err());
    assert!(!dir.join("new.wav").exists());
    assert!(!std::env::temp_dir().join("new.wav").exists());

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn rename_recording_rejects_a_non_wav_extension() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_rename_ext");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("old.wav"), &[1, 2]);

    let result = rename_recording(&dir, "old.wav", "new.mp3");
    assert!(result.is_err());

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn rename_recording_rejects_a_collision() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_rename_collision");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("old.wav"), &[1, 2]);
    write_test_wav(&dir.join("existing.wav"), &[3, 4]);

    let result = rename_recording(&dir, "old.wav", "existing.wav");
    assert!(result.is_err());
    assert!(dir.join("old.wav").exists());

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn delete_recording_removes_the_file() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_delete");
    std::fs::create_dir_all(&dir).unwrap();
    write_test_wav(&dir.join("gone.wav"), &[1, 2]);

    let result = delete_recording(&dir, "gone.wav");
    assert!(result.is_ok());
    assert!(!dir.join("gone.wav").exists());

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn delete_recording_rejects_a_path_separator() {
    let dir = std::env::temp_dir().join("pr7_recordings_test_delete_sep");
    std::fs::create_dir_all(&dir).unwrap();

    let result = delete_recording(&dir, "../escape.wav");
    assert!(result.is_err());

    std::fs::remove_dir_all(&dir).ok();
  }
}
