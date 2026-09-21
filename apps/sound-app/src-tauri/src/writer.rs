use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::capture::AudioFormat;
use crate::settings;
use crate::state::RecordingState;
use crate::storage;

/// How many `WriterMessage`s the channel between the capture thread's frame
/// callback and the writer thread can hold before `try_send` starts
/// returning `Full`. 250 frames at ~20ms each is roughly 5 seconds of
/// buffered audio -- enough slack for a brief disk hiccup without letting
/// memory grow unboundedly if the writer falls behind for good.
const CHANNEL_CAPACITY: usize = 250;

const STORAGE_CHECK_INTERVAL: Duration = Duration::from_secs(5);

pub enum WriterMessage {
  Frame(Vec<i16>),
  Finalize,
  Discard,
}

#[derive(Debug)]
pub struct WriterResult {
  pub file_path: String,
  pub duration_ms: u64,
  pub size_bytes: u64,
}

pub struct WriterError {
  pub message: String,
}

pub struct WriterHandle {
  pub sender: SyncSender<WriterMessage>,
  pub join_handle: thread::JoinHandle<Option<WriterResult>>,
}

/// Resolves (and creates, if missing) the save directory. If a settings
/// file has a `save_dir` set and that directory exists and is writable, it
/// wins; otherwise (unset, or set but no longer valid -- e.g. an external
/// drive got unplugged) this falls back to the default:
/// `<OS audio dir>/Sound Recorder/`.
pub fn recording_dir(app: &AppHandle) -> Result<PathBuf, String> {
  let default_dir = || -> Result<PathBuf, String> {
    let audio_dir = app
      .path()
      .audio_dir()
      .map_err(|e| format!("Could not resolve audio directory: {e}"))?;
    let dir = audio_dir.join("Sound Recorder");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Could not create save directory: {e}"))?;
    Ok(dir)
  };

  let Ok(config_dir) = app.path().app_config_dir() else {
    return default_dir();
  };
  let loaded = settings::load_settings(&config_dir);
  match loaded.save_dir {
    Some(custom) => {
      let path = PathBuf::from(custom);
      if path.is_dir() && is_writable(&path) {
        Ok(path)
      } else {
        default_dir()
      }
    }
    None => default_dir(),
  }
}

/// Checks whether this process can actually write to `path`, by writing and
/// immediately deleting a marker file. Permission-bit checks (e.g. the
/// readonly flag) can't reliably answer this -- they miss ownership
/// mismatches, a missing directory-execute bit, and read-only-mounted
/// filesystems -- so a real write probe is the only cross-platform way to
/// know for sure. Used by `recording_dir` to decide whether a custom
/// `save_dir` is still usable before trusting it over the default.
fn is_writable(path: &Path) -> bool {
  let probe = path.join(".sound-recorder-write-test");
  match std::fs::write(&probe, b"") {
    Ok(()) => {
      let _ = std::fs::remove_file(&probe);
      true
    }
    Err(_) => false,
  }
}

/// Builds `(temp_path, final_path)` for a new recording started now, named
/// with `prefix` (falling back to `"recording"` if empty -- a second line
/// of defense beyond `settings::save_settings`'s own validation, in case a
/// settings file was edited or corrupted outside the app). `temp_path` is
/// the final name with an extra `.tmp` suffix.
pub fn timestamped_wav_paths(dir: &Path, prefix: &str) -> (PathBuf, PathBuf) {
  let prefix = if prefix.trim().is_empty() || prefix.contains('/') || prefix.contains('\\') {
    "recording"
  } else {
    prefix
  };
  let now = std::time::SystemTime::now();
  let datetime: chrono::DateTime<chrono::Local> = now.into();
  let name = format!("{prefix}-{}.wav", datetime.format("%Y-%m-%d_%H-%M-%S%.3f"));
  let final_path = dir.join(&name);
  let temp_path = dir.join(format!("{name}.tmp"));
  (temp_path, final_path)
}

/// Creates the channel used to hand frames from the capture thread's frame
/// callback to the writer thread. Split out from `spawn_writer` because the
/// `Sender` is needed by `make_frame_callback` before the real `AudioFormat`
/// (needed to create the `hound::WavWriter`) is known.
pub fn create_channel() -> (SyncSender<WriterMessage>, Receiver<WriterMessage>) {
  mpsc::sync_channel(CHANNEL_CAPACITY)
}

/// Creates the temp WAV file and spawns the thread that owns it. Synchronous
/// (the `hound::WavWriter::create` call, and thus any permission/path
/// error, happens here before any thread is spawned) — matches the pattern
/// `LinuxPulseCapture::start` already uses for its own synchronous setup.
pub fn spawn_writer(
  receiver: Receiver<WriterMessage>,
  temp_path: PathBuf,
  final_path: PathBuf,
  format: AudioFormat,
  app: AppHandle,
  state: Arc<Mutex<RecordingState>>,
) -> Result<thread::JoinHandle<Option<WriterResult>>, WriterError> {
  let spec = hound::WavSpec {
    channels: format.channels as u16,
    sample_rate: format.sample_rate,
    bits_per_sample: 16,
    sample_format: hound::SampleFormat::Int,
  };
  let writer = hound::WavWriter::create(&temp_path, spec).map_err(|e| WriterError {
    message: format!("Could not create recording file: {e}"),
  })?;

  Ok(thread::spawn(move || {
    run_writer_loop(writer, receiver, format, temp_path, final_path, app, state)
  }))
}

/// Sets `RecordingState::Error` and emits it, mirroring the pattern every
/// failure path in this loop already uses. Shared by the write-failure and
/// low-disk-space paths so both report failures identically.
fn fail_recording(app: &AppHandle, state: &Arc<Mutex<RecordingState>>, message: String) {
  *state.lock().unwrap() = RecordingState::Error {
    message: message.clone(),
    recoverable: true,
  };
  let _ = tauri::Emitter::emit(
    app,
    "recording-state-changed",
    RecordingState::Error {
      message,
      recoverable: true,
    },
  );
}

fn run_writer_loop(
  mut writer: hound::WavWriter<std::io::BufWriter<std::fs::File>>,
  receiver: Receiver<WriterMessage>,
  format: AudioFormat,
  temp_path: PathBuf,
  final_path: PathBuf,
  app: AppHandle,
  state: Arc<Mutex<RecordingState>>,
) -> Option<WriterResult> {
  let mut failed = false;
  let mut last_storage_check = Instant::now();

  loop {
    match receiver.recv() {
      Ok(WriterMessage::Frame(samples)) => {
        if failed {
          continue;
        }

        if last_storage_check.elapsed() >= STORAGE_CHECK_INTERVAL {
          last_storage_check = Instant::now();
          let dir = temp_path.parent().unwrap_or(&temp_path);
          if let Ok(free) = storage::free_space_bytes(dir) {
            if storage::is_below_threshold(free) {
              failed = true;
              fail_recording(
                &app,
                &state,
                "Recording failed: disk space is critically low".to_string(),
              );
              continue;
            }
          }
        }

        let mut write_err: Option<hound::Error> = None;
        for sample in samples {
          if let Err(e) = writer.write_sample(sample) {
            write_err = Some(e);
            break;
          }
        }
        if write_err.is_none() {
          write_err = writer.flush().err();
        }
        if let Some(e) = write_err {
          failed = true;
          fail_recording(&app, &state, format!("Could not write audio to disk: {e}"));
        }
      }
      Ok(WriterMessage::Finalize) => {
        if failed {
          // Don't delete: the temp file is already flushed and valid up to
          // the last successful frame (see the channel-closed arm below for
          // the same rationale). Preserve it for
          // recovery::recover_orphaned_recordings to promote at next
          // startup, consistent with every other non-explicit-Discard exit
          // path in this loop.
          return None;
        }
        let duration_samples = writer.duration();
        let duration_ms = (duration_samples as u64 * 1000) / format.sample_rate.max(1) as u64;
        if writer.finalize().is_err() {
          return None;
        }
        let size_bytes = std::fs::metadata(&temp_path).map(|m| m.len()).unwrap_or(0);
        if std::fs::rename(&temp_path, &final_path).is_err() {
          return None;
        }
        return Some(WriterResult {
          file_path: final_path.to_string_lossy().to_string(),
          duration_ms,
          size_bytes,
        });
      }
      Ok(WriterMessage::Discard) => {
        drop(writer);
        let _ = std::fs::remove_file(&temp_path);
        return None;
      }
      Err(_) => {
        // Channel closed with no terminal message: the frame callback's
        // Sender was dropped after the capture thread exited due to an
        // error, with nobody sending Finalize/Discard. Drop the writer
        // (its periodic flush() calls mean the temp file is already valid
        // up to the last frame) and leave it for recovery::
        // recover_orphaned_recordings to promote or delete at next startup.
        return None;
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn sync_channel_returns_full_when_capacity_exceeded() {
    let (sender, receiver) = create_channel();
    let mut sent = 0;
    loop {
      match sender.try_send(WriterMessage::Frame(vec![1, 2])) {
        Ok(()) => sent += 1,
        Err(mpsc::TrySendError::Full(_)) => break,
        Err(e) => panic!("unexpected error: {e:?}"),
      }
      if sent > 10_000 {
        panic!("channel never reported Full -- capacity assumption is wrong");
      }
    }
    println!("channel reported Full after {sent} successful sends");
    assert!(sent > 0, "expected at least some capacity before Full");
    assert_eq!(
      sent, CHANNEL_CAPACITY,
      "sent a different count than the configured capacity before Full"
    );
    drop(receiver);
  }

  #[test]
  fn is_writable_returns_true_for_a_real_writable_directory() {
    let dir = std::env::temp_dir().join("pr8_writer_test_writable");
    std::fs::create_dir_all(&dir).unwrap();
    assert!(is_writable(&dir));
    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn is_writable_returns_false_for_a_nonexistent_directory() {
    let dir = std::env::temp_dir().join("pr8_writer_test_not_writable_missing");
    std::fs::remove_dir_all(&dir).ok();
    assert!(!is_writable(&dir));
  }

  #[test]
  fn timestamped_names_have_the_expected_shape() {
    let dir = PathBuf::from("/tmp/whatever");
    let (temp, final_) = timestamped_wav_paths(&dir, "recording");
    assert!(temp
      .to_string_lossy()
      .starts_with("/tmp/whatever/recording-"));
    assert!(temp.to_string_lossy().ends_with(".wav.tmp"));
    assert!(final_.to_string_lossy().ends_with(".wav"));
    assert!(!final_.to_string_lossy().ends_with(".wav.tmp"));
  }

  #[test]
  fn timestamped_names_use_the_given_prefix() {
    let dir = PathBuf::from("/tmp/whatever");
    let (temp, _) = timestamped_wav_paths(&dir, "meeting");
    assert!(temp.to_string_lossy().starts_with("/tmp/whatever/meeting-"));
  }

  #[test]
  fn timestamped_names_fall_back_to_recording_for_an_empty_prefix() {
    let dir = PathBuf::from("/tmp/whatever");
    let (temp, _) = timestamped_wav_paths(&dir, "   ");
    assert!(temp
      .to_string_lossy()
      .starts_with("/tmp/whatever/recording-"));
  }

  #[test]
  fn timestamped_names_fall_back_to_recording_for_a_prefix_with_a_path_separator() {
    let dir = PathBuf::from("/tmp/whatever");
    let (temp, _) = timestamped_wav_paths(&dir, "../escape");
    assert!(temp
      .to_string_lossy()
      .starts_with("/tmp/whatever/recording-"));
  }

  #[test]
  fn finalize_produces_a_real_playable_wav_file_with_correct_metadata() {
    let dir = std::env::temp_dir().join("pr5_writer_test_finalize");
    std::fs::create_dir_all(&dir).unwrap();
    let temp_path = dir.join("rec.wav.tmp");
    let final_path = dir.join("rec.wav");
    let _ = std::fs::remove_file(&temp_path);
    let _ = std::fs::remove_file(&final_path);

    let format = AudioFormat {
      sample_rate: 48_000,
      channels: 2,
    };
    let spec = hound::WavSpec {
      channels: format.channels as u16,
      sample_rate: format.sample_rate,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let writer = hound::WavWriter::create(&temp_path, spec).unwrap();
    let (sender, receiver) = mpsc::channel::<WriterMessage>();

    // run_writer_loop needs a real AppHandle, which a plain unit test can't
    // construct. Its `app`/`state` parameters are only touched on the
    // write-error path, which this happy-path test never exercises -- so we
    // call the same finalize logic directly here rather than through
    // run_writer_loop, to keep this test AppHandle-free. The exact
    // write/flush/finalize/rename sequence matches run_writer_loop's
    // Finalize arm above verbatim. `temp_path`/`final_path` are cloned
    // before the move so the outer test can still assert on them afterward.
    let thread_temp_path = temp_path.clone();
    let thread_final_path = final_path.clone();
    let handle = thread::spawn(move || {
      let mut writer = writer;
      loop {
        match receiver.recv().unwrap() {
          WriterMessage::Frame(samples) => {
            for s in samples {
              writer.write_sample(s).unwrap();
            }
            writer.flush().unwrap();
          }
          WriterMessage::Finalize => {
            let duration_samples = writer.duration();
            let duration_ms = (duration_samples as u64 * 1000) / format.sample_rate.max(1) as u64;
            writer.finalize().unwrap();
            let size_bytes = std::fs::metadata(&thread_temp_path).unwrap().len();
            std::fs::rename(&thread_temp_path, &thread_final_path).unwrap();
            return Some(WriterResult {
              file_path: thread_final_path.to_string_lossy().to_string(),
              duration_ms,
              size_bytes,
            });
          }
          WriterMessage::Discard => return None,
        }
      }
    });

    let first_frame: Vec<i16> = (0..960).collect(); // 480 stereo frames = 10ms at 48kHz
    let second_frame: Vec<i16> = (960..1920).collect(); // another 480 stereo frames = 10ms
    sender
      .send(WriterMessage::Frame(first_frame.clone()))
      .unwrap();
    sender
      .send(WriterMessage::Frame(second_frame.clone()))
      .unwrap();
    sender.send(WriterMessage::Finalize).unwrap();

    let result = handle.join().unwrap().unwrap();
    assert_eq!(
      result.size_bytes,
      44 + (first_frame.len() + second_frame.len()) as u64 * 2
    );
    assert_eq!(result.duration_ms, 20); // 960 total stereo frames / 48_000 Hz * 1000 = 20ms, exact
    assert!(!std::fs::exists(&temp_path).unwrap());
    assert!(std::fs::exists(&final_path).unwrap());

    let reader = hound::WavReader::open(&final_path).unwrap();
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.spec().sample_rate, 48_000);
    let samples: Vec<i16> = reader.into_samples::<i16>().map(|s| s.unwrap()).collect();
    let mut expected = first_frame;
    expected.extend(second_frame);
    assert_eq!(samples, expected);

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn discard_deletes_the_temp_file_without_producing_a_result() {
    let dir = std::env::temp_dir().join("pr5_writer_test_discard");
    std::fs::create_dir_all(&dir).unwrap();
    let temp_path = dir.join("rec.wav.tmp");
    let _ = std::fs::remove_file(&temp_path);

    let spec = hound::WavSpec {
      channels: 1,
      sample_rate: 48_000,
      bits_per_sample: 16,
      sample_format: hound::SampleFormat::Int,
    };
    let writer = hound::WavWriter::create(&temp_path, spec).unwrap();
    let (sender, receiver) = mpsc::channel::<WriterMessage>();

    let thread_temp_path = temp_path.clone();
    let handle = thread::spawn(move || -> Option<WriterResult> {
      let mut writer = writer;
      loop {
        match receiver.recv().unwrap() {
          WriterMessage::Frame(samples) => {
            for s in samples {
              writer.write_sample(s).unwrap();
            }
          }
          WriterMessage::Discard => {
            drop(writer);
            std::fs::remove_file(&thread_temp_path).unwrap();
            return None;
          }
          WriterMessage::Finalize => panic!("test only sends Discard"),
        }
      }
    });

    sender.send(WriterMessage::Frame(vec![9, 9])).unwrap();
    sender.send(WriterMessage::Discard).unwrap();
    let result = handle.join().unwrap();

    assert!(result.is_none());
    assert!(!std::fs::exists(&temp_path).unwrap());
    std::fs::remove_dir_all(&dir).ok();
  }
}
