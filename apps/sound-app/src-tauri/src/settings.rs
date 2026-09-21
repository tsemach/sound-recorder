use std::path::{Path, PathBuf};

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(default)]
pub struct Settings {
  pub save_dir: Option<String>,
  pub filename_prefix: String,
  pub default_source_id: Option<String>,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      save_dir: None,
      filename_prefix: "recording".to_string(),
      default_source_id: None,
    }
  }
}

fn settings_path(config_dir: &Path) -> PathBuf {
  config_dir.join("settings.json")
}

/// Falls back to `Settings::default()` on any failure (missing file,
/// unreadable, invalid JSON) -- a corrupt or absent settings file must
/// never block the app from starting or recording.
pub fn load_settings(config_dir: &Path) -> Settings {
  std::fs::read_to_string(settings_path(config_dir))
    .ok()
    .and_then(|contents| serde_json::from_str(&contents).ok())
    .unwrap_or_default()
}

fn validate_prefix(prefix: &str) -> Result<(), String> {
  if prefix.trim().is_empty() {
    return Err("Filename prefix cannot be empty".to_string());
  }
  if prefix.contains('/') || prefix.contains('\\') {
    return Err("Filename prefix cannot contain a path separator".to_string());
  }
  Ok(())
}

pub fn save_settings(config_dir: &Path, settings: &Settings) -> Result<(), String> {
  validate_prefix(&settings.filename_prefix)?;
  std::fs::create_dir_all(config_dir)
    .map_err(|e| format!("Could not create settings directory: {e}"))?;
  let json = serde_json::to_string_pretty(settings)
    .map_err(|e| format!("Could not serialize settings: {e}"))?;
  std::fs::write(settings_path(config_dir), json)
    .map_err(|e| format!("Could not write settings file: {e}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn load_settings_returns_defaults_when_the_file_is_missing() {
    let dir = std::env::temp_dir().join("pr8_settings_test_missing");
    std::fs::remove_dir_all(&dir).ok();

    let settings = load_settings(&dir);
    assert_eq!(settings.save_dir, None);
    assert_eq!(settings.filename_prefix, "recording");
    assert_eq!(settings.default_source_id, None);
  }

  #[test]
  fn load_settings_returns_defaults_when_the_file_is_corrupt() {
    let dir = std::env::temp_dir().join("pr8_settings_test_corrupt");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(settings_path(&dir), b"not valid json").unwrap();

    let settings = load_settings(&dir);
    assert_eq!(settings.save_dir, None);

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn save_then_load_round_trips_real_values() {
    let dir = std::env::temp_dir().join("pr8_settings_test_roundtrip");
    std::fs::remove_dir_all(&dir).ok();

    let settings = Settings {
      save_dir: Some("/home/user/MyRecordings".to_string()),
      filename_prefix: "meeting".to_string(),
      default_source_id: Some("source-42".to_string()),
    };
    save_settings(&dir, &settings).unwrap();

    let loaded = load_settings(&dir);
    assert_eq!(loaded.save_dir, settings.save_dir);
    assert_eq!(loaded.filename_prefix, settings.filename_prefix);
    assert_eq!(loaded.default_source_id, settings.default_source_id);

    std::fs::remove_dir_all(&dir).ok();
  }

  #[test]
  fn save_settings_rejects_an_empty_prefix() {
    let dir = std::env::temp_dir().join("pr8_settings_test_empty_prefix");
    std::fs::remove_dir_all(&dir).ok();

    let settings = Settings {
      save_dir: None,
      filename_prefix: "   ".to_string(),
      default_source_id: None,
    };
    let result = save_settings(&dir, &settings);
    assert!(result.is_err());
    assert!(!settings_path(&dir).exists());
  }

  #[test]
  fn save_settings_rejects_a_path_separator_in_the_prefix() {
    let dir = std::env::temp_dir().join("pr8_settings_test_prefix_sep");
    std::fs::remove_dir_all(&dir).ok();

    let settings = Settings {
      save_dir: None,
      filename_prefix: "../escape".to_string(),
      default_source_id: None,
    };
    let result = save_settings(&dir, &settings);
    assert!(result.is_err());
  }

  #[test]
  fn save_settings_succeeds_with_the_default_settings() {
    let dir = std::env::temp_dir().join("pr8_settings_test_default_save");
    std::fs::remove_dir_all(&dir).ok();

    let result = save_settings(&dir, &Settings::default());
    assert!(result.is_ok());

    std::fs::remove_dir_all(&dir).ok();
  }
}
