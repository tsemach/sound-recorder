mod capture;
mod commands;
mod recovery;
mod state;
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
      Ok(())
    })
    .manage(SharedState::new(Box::new(LinuxPulseCapture::new())))
    .invoke_handler(tauri::generate_handler![
      commands::list_sources,
      commands::start_recording,
      commands::pause_recording,
      commands::resume_recording,
      commands::stop_recording,
      commands::cancel_recording,
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
