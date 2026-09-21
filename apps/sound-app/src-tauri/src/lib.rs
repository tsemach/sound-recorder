mod capture;
mod commands;
mod recordings;
mod recovery;
mod settings;
mod state;
mod storage;
mod tick;
mod writer;

use capture::linux_pulse::LinuxPulseCapture;
use state::SharedState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  tauri::Builder::default()
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      match writer::recording_dir(app.handle()) {
        Ok(dir) => {
          if let Err(e) = recovery::recover_orphaned_recordings(&dir) {
            log::warn!("Could not recover orphaned recordings: {e}");
          }
        }
        Err(e) => log::warn!("Could not resolve save directory for recovery: {e}"),
      }
      Ok(())
    })
    .plugin(tauri_plugin_opener::init())
    .manage(SharedState::new(Box::new(LinuxPulseCapture::new())))
    .invoke_handler(tauri::generate_handler![
      commands::list_sources,
      commands::start_recording,
      commands::pause_recording,
      commands::resume_recording,
      commands::stop_recording,
      commands::cancel_recording,
      commands::list_recordings,
      commands::rename_recording,
      commands::delete_recording,
      commands::get_settings,
      commands::update_settings,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
