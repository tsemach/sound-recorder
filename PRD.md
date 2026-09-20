 # Product Requirements Document: Sound Recorder

## 1. Product Summary

Sound Recorder is a pair of local-first applications that capture digital audio currently playing on a device and save it as an audio file:

- **sound-app:** a desktop application implemented with Tauri.
- **mobile-app:** a mobile application implemented with React Native.

Capture is available only where the operating system and source application permit it. The products must not bypass DRM, secure audio paths, or platform restrictions.

## 2. Goals

- Record permitted system playback digitally, without requiring an external microphone.
- Provide simple start, pause, resume, and stop controls.
- Save every completed recording as a local file.
- Clearly show recording status, duration, source, and errors.
- Protect user privacy by keeping recordings on the device in the initial release.

## 3. Non-goals

- Microphone recording in the initial release.
- Cloud sync, accounts, or online audio processing.
- Audio editing beyond basic recording controls.
- Circumventing DRM, copyright controls, or operating-system limitations.

## 4. Target Users

- Users recording audio they own or are authorized to capture.
- Developers and testers diagnosing permitted audio output.
- Content creators archiving permitted playback for later use.

## 5. User Stories

- As a user, I can see whether a supported playback source is available.
- As a user, I can start, pause, resume, and stop a recording.
- As a user, I can choose or confirm the output location and format.
- As a user, I can find, play, rename, export, and delete saved recordings.
- As a user, I receive a clear explanation when capture is unavailable or blocked.

## 6. Functional Requirements

### 6.1 Shared Requirements

- Provide states for idle, preparing, recording, paused, saving, saved, and error.
- Display elapsed time and an audio-level indicator where supported.
- Do not record until the user explicitly starts recording.
- Save each recording with a unique timestamped filename.
- Write audio incrementally so long recordings do not require loading into memory.
- Validate available storage before and during recording.
- Save recordings locally and retain them after app restart.
- Provide a recordings list with date, duration, format, and file size.
- Confirm cancellation and deletion to prevent accidental data loss.
- Request only necessary permissions and explain each request.

### 6.2 sound-app: Desktop

- Use Tauri with a secure Rust backend for capture, file writing, and device access.
- Capture system output audio on supported Windows, macOS, and Linux configurations.
- Detect available output devices and allow source selection where supported.
- Provide a native folder picker and remember the last save location.
- Support at least one lossless or high-quality format in the MVP, such as WAV or Opus.
- Continue recording when minimized where supported by the operating system.
- Provide tray or menu-bar controls where supported.
- Use restrictive Tauri capabilities and avoid unnecessary network, shell, or filesystem access.

### 6.3 mobile-app: Mobile

- Use React Native with native modules or approved platform APIs for playback capture.
- Support current, agreed-upon iOS and Android versions; document platform differences.
- Detect and explain devices or source apps that do not permit internal-audio capture.
- Handle phone calls, audio focus changes, app suspension, lock screen behavior, and low storage.
- Save files in app-accessible local storage and provide platform-appropriate export/share actions.
- Show the platform-required recording indicator or notification.
- Request audio/media permissions at the point of need and provide recovery guidance if denied.
- Stop safely and preserve a recoverable partial file when the operating system interrupts capture.

## 7. User Experience

### Main Screen

- Show the selected source and capture availability.
- Provide a prominent Record button.
- Show Pause/Resume and Stop only when applicable.
- Display elapsed time, audio level, storage availability, and current status.
- Link to recordings and settings.

### Recordings Screen

- List recordings newest first.
- Support play, rename, delete, export/share, and open/reveal actions where available.
- Confirm destructive deletion.

### Settings

- Output format, quality, channels, and default save location where supported.
- Source/device selection where supported.
- Filename template and privacy/permission guidance.

## 8. Technical Requirements

- Desktop implementation: Tauri, Rust backend, and a web-based frontend.
- Mobile implementation: React Native with platform-specific native capture integration.
- Isolate platform-specific capture behind a common recording interface.
- Use temporary files and atomically rename finalized files to prevent corrupt visible files.
- Recover or clean up temporary files after crashes.
- Keep audio processing and storage local by default.
- Preserve correct sample rate, channels, duration, and container metadata.

## 9. Privacy, Legal, and Safety

- Never start recording silently or automatically.
- Clearly indicate active recording in the UI and through required platform indicators.
- Tell users they are responsible for permission to record and use audio.
- Do not bypass DRM, secure playback, or source-app restrictions.
- Do not upload audio. Optional diagnostics must exclude audio and require consent.

## 10. Error Handling

Provide actionable errors for denied permissions, missing sources, unavailable devices, blocked capture, unsupported configurations, insufficient storage, failed writes, interruptions, and unsupported formats. When possible, preserve a partial recording rather than silently discarding it.

## 11. Quality Requirements

- Recording controls respond within 500 ms under normal conditions.
- The application remains responsive during long recordings and uses bounded memory.
- Recordings are playable and synchronized on supported hardware with sufficient storage.
- Desktop controls support keyboard and screen readers; mobile controls support accessibility services.
- UI strings are localization-ready and support platform light/dark themes where practical.

## 12. MVP Scope

- Tauri desktop app and React Native mobile app.
- Supported system-playback capture with documented platform limitations.
- Start, pause, resume, stop, and cancel controls.
- Local file saving with timestamped names.
- Recording list with playback, export/share where supported, and deletion.
- Permission, storage, device, interruption, and capture-error handling.

## 13. Future Enhancements

- Waveform display and trimming.
- Scheduled recordings.
- More codecs and export presets.
- Optional cloud backup.
- Cross-device transfer and recording folders.

## 14. Acceptance Criteria

1. On a supported desktop configuration, a user can select a source, record currently playing system audio, stop, and open a valid saved file.
2. On a supported mobile configuration, a user can grant required permissions, record permitted playback audio, stop, and access a valid saved file.
3. Pause and resume produce one playable file with the expected duration.
4. Saved recordings remain available after application restart.
5. Unsupported or blocked capture is reported clearly without claiming a recording was made.
6. Recording never starts without explicit user action and is visibly indicated while active.
7. Failed saves provide an actionable error and do not silently discard recoverable data.

## 15. Open Decisions

- Minimum supported desktop operating systems and mobile OS versions.
- Tauri frontend framework.
- Supported formats and default quality for each platform.
- Exact background-recording behavior on each mobile platform.
- Whether microphone capture will be added in a later release.
