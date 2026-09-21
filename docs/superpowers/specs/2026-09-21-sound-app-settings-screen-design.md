# sound-app: Settings Screen (PR 8 of 8)

Status: approved for planning
Scope: `apps/sound-app` only. Depends on PR 7 (recordings list screen, merged to master). This is the final PR of the 8-PR sound-app Tauri migration.

## Context

Every other screen in the app is now real: recording, real capture, a real
WAV writer, storage/error handling, and a recordings list. The one thing
still hardcoded is *where* and *how* recordings are saved, and there is no
way for the user to influence it. This PR adds a Settings screen closing
that gap.

Key PRD requirements this PR must honor:
- §6.2 (sound-app): "Provide a native folder picker and remember the last
  save location."
- §7 (Settings): "Output format, quality, channels, and default save
  location where supported." / "Source/device selection where supported."
  / "Filename template and privacy/permission guidance."
- §7 (Main Screen): "Link to recordings and settings" (recordings' link
  shipped in PR7; this PR ships settings').
- Original migration spec's roadmap entry for this PR: "save-location
  picker (native dialog, remembered via Tauri `Store`), filename template,
  source/quality selection persisted."

### Decisions made during design

- **No quality/format controls.** Capture always mirrors the selected
  source's native `AudioFormat` (sample rate, channels) — there is no
  existing capability anywhere in the capture pipeline to override this,
  and the migration's own stated scope explicitly excludes adding
  non-WAV formats (Opus, etc.). Building real quality control would mean
  new capture-layer resampling/bit-depth-conversion capability, which is
  meaningfully larger scope than a settings screen and not something
  anything in the app currently needs. The PRD's "where supported" carve-out
  covers this: it isn't supported today, so it's omitted.
- **Filename template is an editable prefix, not a full token-based
  pattern.** The current fixed scheme is `recording-YYYY-MM-DD_HH-MM-SS.
  mmm.wav`; PR7 already made the recordings list recognize *any* `*.wav`
  file (not just that prefix), so changing the prefix is safe and doesn't
  orphan old recordings from the list. The user edits only the prefix
  (default `"recording"`); the timestamp suffix is always appended
  automatically, guaranteeing uniqueness regardless of what the user
  types. A full rearrangeable token template would add real validation
  surface (character escaping, uniqueness guarantees, valid-filename
  checks per platform) for a feature nothing in the PRD's user stories
  actually asks for beyond "a filename template."
- **Persistence is a hand-rolled typed JSON file + two commands, not
  `tauri-plugin-store`.** The original migration roadmap doc speculatively
  named `tauri-plugin-store` back when this PR was first sketched, before
  PR6/PR7 established this project's stronger, later-formed preference:
  narrow, typed custom commands (`list_recordings`, `rename_recording`,
  `delete_recording`) instead of exposing a generic API surface to the
  frontend. A hand-rolled `settings.rs` (mirroring `recordings.rs`'s
  established shape) needs zero new Tauri capability grants — the frontend
  never touches the filesystem or a store directly, matching the PRD's own
  "use restrictive Tauri capabilities" principle even more tightly than a
  plugin-based store would (a store plugin's frontend JS API is itself a
  new IPC-exposed capability surface; two narrow custom commands are not).
- **`tauri-plugin-dialog` is required and is not a persistence-mechanism
  choice** — it's the only reasonable way to get a *native* OS folder
  picker (the PRD's explicit requirement, not a text-entry field), and is
  orthogonal to how the chosen path then gets persisted.
- **Every persisted value degrades gracefully to a working default rather
  than erroring**, matching this app's established philosophy (PR7's
  `recordings.rs` skips one corrupt file instead of aborting the whole
  scan; `recovery.rs` cleans up orphaned temp files rather than leaving the
  app stuck): a missing/corrupt settings file falls back to
  `Settings::default()`; a persisted save directory that no longer exists
  or isn't writable falls back to the default `<OS audio dir>/Sound
  Recorder` directory; a persisted default source that's no longer present
  in `list_sources()`'s results falls back to whatever source is first in
  that list. None of these are startup errors or block recording.
- **No router; a third view.** `App.tsx`'s existing `view: "recorder" |
  "recordings"` state (from PR7) gains a third value, `"settings"`. Same
  no-router pattern as PR7 — a full router is still unnecessary complexity
  for a three-screen desktop app.

## Architecture

### Module structure (`apps/sound-app/src-tauri/src/`)

```
settings.rs   — Settings struct, load_settings/save_settings, prefix
                validation (mirrors recordings.rs's shape: domain logic +
                real-file tests, no Tauri types inside the module itself)
commands.rs   — thin #[tauri::command] wrappers (get_settings,
                update_settings) delegating to settings.rs, matching this
                crate's existing thin-command pattern
```

### `settings.rs`

```rust
use std::path::PathBuf;

#[derive(serde::Serialize, serde::Deserialize, Default, Clone)]
pub struct Settings {
  pub save_dir: Option<String>,
  pub filename_prefix: String,
  pub default_source_id: Option<String>,
}

pub fn settings_path(config_dir: &std::path::Path) -> PathBuf {
  config_dir.join("settings.json")
}

pub fn load_settings(config_dir: &std::path::Path) -> Settings {
  // Reads settings_path(config_dir); any failure (missing file, unreadable,
  // invalid JSON) falls back to Settings::default() rather than erroring --
  // a corrupt or absent settings file must never block the app.
}

pub fn save_settings(config_dir: &std::path::Path, settings: &Settings) -> Result<(), String> {
  // Validates filename_prefix (see validate_prefix below) before writing.
  // Creates config_dir if missing, writes settings_path(config_dir) as
  // pretty JSON.
}

fn validate_prefix(prefix: &str) -> Result<(), String> {
  // Rejects empty/whitespace-only, and rejects a path separator (the same
  // defense-in-depth already established for recordings.rs's rename/delete
  // name validation, since this value ends up embedded directly in a
  // filesystem path).
}
```

`commands.rs` gains two thin `#[tauri::command]` wrappers
(`get_settings`, `update_settings`) that resolve `app.path().
app_config_dir()` and delegate to the functions above, returning
`Result<_, CommandError>` matching every other command in this file.
`get_settings` never fails (returns `Settings::default()` on any load
problem, per the module's own fallback behavior); `update_settings`
can fail only on a real validation error or write failure.

### Integration: `writer.rs`

```rust
pub fn recording_dir(app: &AppHandle) -> Result<PathBuf, String> {
  // Same self-contained shape as today (no new parameter -- it already
  // internally resolves app.path().audio_dir()), but now also internally
  // calls settings::load_settings(&app.path().app_config_dir()...) and
  // applies this resolution order:
  // 1. If settings.save_dir is Some(path) and that path exists and is
  //    writable, use it.
  // 2. Otherwise (unset, or set but no longer valid), fall back to the
  //    existing default: <OS audio dir>/Sound Recorder.
  // Either way, the resolved directory is created if missing, exactly as
  // today.
}

pub fn timestamped_wav_paths(dir: &Path, prefix: &str) -> (PathBuf, PathBuf) {
  // Same shape as today, but the hardcoded "recording" literal becomes the
  // `prefix` parameter (empty prefix from a corrupt/unvalidated settings
  // value falls back to "recording" here too, as a second line of
  // defense beyond settings.rs's own validation).
}
```

`recording_dir` stays self-contained (it already internally resolves
`audio_dir()` with no parameters; loading settings internally the same
way is consistent, not a new pattern). `timestamped_wav_paths` has no
`AppHandle`, so its one caller — `commands.rs`'s `start_recording` —
loads settings once via `settings::load_settings` and passes just the
`filename_prefix` field through. Settings are not cached in
`SharedState`: they can change between recordings, and there's no
performance reason to avoid a cheap file read once per `start_recording`
call.

### Capability: `tauri-plugin-dialog`

Added the same way `tauri-plugin-opener` was added in PR7: `tauri-plugin-
dialog = "2"` in `Cargo.toml`, `@tauri-apps/plugin-dialog` in
`package.json`, `.plugin(tauri_plugin_dialog::init())` in `lib.rs`, and
the narrowest capability permission that covers a directory-picker dialog
(`dialog:allow-open`) added to `capabilities/default.json` — nothing
broader (no `dialog:default`, no save/message/ask permissions this app
doesn't use).

### Frontend

- `App.tsx`'s `view` state becomes `"recorder" | "recordings" |
  "settings"`. The header's single flip-flop toggle button (PR7) becomes a
  small set of nav controls covering all three views (exact visual
  treatment is an implementation-time call, following this app's existing
  button/spacing conventions — not prescribed here down to the pixel).
- `SettingsScreen.tsx`: on mount, calls `invoke("get_settings")` and
  populates local form state. A "Choose Folder…" button calls `open({
  directory: true })` from `@tauri-apps/plugin-dialog`; on a non-null
  result, immediately calls `invoke("update_settings", { ...current,
  saveDir: result })` and updates the displayed path. A text input for
  `filenamePrefix`, saved on blur or an explicit "Save" action (exact UX
  detail resolved at implementation time — validated against the real
  `Button`/`input` conventions already used in `RecordingsList.tsx`). A
  short static paragraph with the privacy/permission note (PRD §9's
  language: recordings stay local, the user is responsible for permission
  to record).
- The source `<select>` in the recorder view (`App.tsx`, existing since
  PR3) gains a mount-time default: if `Settings.default_source_id` is
  present in the fetched `sources` list, it becomes the initial
  `sourceOverride`; otherwise the existing `sources[0]?.id` fallback
  applies unchanged. Changing the selection also calls
  `invoke("update_settings", { ...current, defaultSourceId: newId })` so
  the choice persists for next launch.

## Testing strategy

- **Rust**: `settings.rs` unit tests using real temp-directory JSON files
  (matching this project's established real-file test style from
  `recordings.rs`/`writer.rs`) — verifying `load_settings` returns
  defaults for a missing file, returns defaults (not a panic/error) for a
  corrupt/invalid-JSON file, round-trips a real save→load; `save_settings`
  rejects an empty/whitespace prefix and a prefix containing a path
  separator; `recording_dir`'s fallback behavior when `save_dir` points at
  a nonexistent directory; `timestamped_wav_paths`'s prefix substitution.
- **Frontend**: Vitest tests for `SettingsScreen` — renders fetched
  settings, wires the folder-picker button to the dialog plugin and
  `update_settings`, wires the prefix field to `update_settings`; updated
  `App.tsx` tests for the 3-way view toggle and the source-default
  behavior (persisted default present vs. absent/stale).
- **Manual**: `pnpm --filter sound-app tauri dev` — open Settings, pick a
  new save folder via the native dialog, confirm a subsequent recording
  actually lands there; change the filename prefix, confirm the next
  recording uses it; change the default source, restart the dev app (or
  reload), confirm it's pre-selected; confirm the privacy note renders;
  confirm graceful fallback by manually corrupting/removing the settings
  file between runs and confirming the app still starts and records
  normally.

## Explicitly out of scope for this PR

- Any quality/format/sample-rate/bit-depth/channel-count control.
- A full, rearrangeable, token-based filename template — prefix only.
- Opus or any non-WAV output format (already out of scope for the whole
  migration).
- Windows/macOS-specific dialog or app-config-directory behavior beyond
  what Tauri's cross-platform APIs already provide.
- Import/export of settings, multiple named profiles, or any
  cloud-synced settings (this app's non-goals already exclude cloud sync
  entirely).
