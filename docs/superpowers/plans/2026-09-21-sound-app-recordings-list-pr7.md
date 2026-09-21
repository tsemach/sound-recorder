# Recordings List Screen (PR 7) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a recordings list screen to `sound-app` that scans the save
directory and lets the user play, rename, reveal-in-file-manager, and delete
(with confirmation) any saved `.wav` recording — newest first, with date,
duration, and size — reachable via a simple in-app view toggle.

**Architecture:** A new `recordings.rs` module owns all filesystem/WAV-header
logic (list/rename/delete), exposed through three thin `#[tauri::command]`
wrappers in `commands.rs` that resolve the save directory and delegate. The
frontend gets a new `RecordingsList.tsx` component that calls those commands
via `invoke`, plays audio through Tauri's asset protocol (`convertFileSrc` +
a native `<audio>` element — no bytes-over-IPC), and reveals files via
`@tauri-apps/plugin-opener`'s `revealItemInDir`. `App.tsx` gains a
`view: "recorder" | "recordings"` toggle — no router.

**Tech Stack:** Tauri v2 (Rust + `tauri::command`), `hound` (already a
dependency, used here for `WavReader::open`/`.duration()`/`.spec()`),
`tauri-plugin-opener` / `@tauri-apps/plugin-opener` v2.5.5, Tauri's asset
protocol (`security.assetProtocol` + `convertFileSrc`), React 19, Vitest.

**Spec:** `docs/superpowers/specs/2026-09-21-sound-app-recordings-list-design.md`

## Global Constraints

- No database, no sidecar metadata files: every list read comes from
  scanning the save directory and reading WAV headers directly (spec's
  "filesystem is the source of truth" principle, carried from the original
  8-PR migration design).
- Recognize **any** `*.wav` file in the save directory as a recording (not
  just files matching the `recording-` prefix) — a file renamed by the user
  must still be listed correctly.
- Exclude `*.wav.tmp` files unconditionally (an in-progress recording's temp
  file, or one that slipped through `recovery.rs`, must never appear).
- A single corrupt/unreadable file must be skipped, not abort the whole scan.
- Tauri capabilities stay minimal and justified: this PR adds exactly one
  new permission (`opener:allow-reveal-item-in-dir`) and one asset-protocol
  scope restricted to `$AUDIO/Sound Recorder/**` — nothing broader.
- `rename_recording`/`delete_recording` reject any `name`/`new_name`
  containing a path separator (`/` or `\`), and `rename_recording` rejects a
  new name that doesn't end in `.wav` or that collides with an existing file.
- No router: navigation is a local `view` state toggle in `App.tsx`, matching
  this app's existing single-page structure.
- Delete requires a native `window.confirm()` guard, matching the existing
  Cancel-recording confirmation pattern already in `App.tsx`.

---

## File Structure

**Create:**
- `apps/sound-app/src-tauri/src/recordings.rs` — `RecordingMeta` struct,
  `list_recordings`/`rename_recording`/`delete_recording`, all filesystem +
  WAV-header logic and its tests.
- `apps/sound-app/src/components/RecordingsList.tsx` — the recordings list
  UI: fetch on mount, per-row play/rename/reveal/delete.
- `apps/sound-app/src/components/RecordingsList.test.tsx` — Vitest tests for
  rendering and button wiring, mocking `invoke`/`convertFileSrc`/
  `revealItemInDir`.

**Modify:**
- `apps/sound-app/src-tauri/src/lib.rs` — register `mod recordings;`, the
  opener plugin, and the three new commands.
- `apps/sound-app/src-tauri/src/commands.rs` — three thin command wrappers.
- `apps/sound-app/src-tauri/Cargo.toml` — add `tauri-plugin-opener`; add the
  `protocol-asset` feature to the existing `tauri` dependency.
- `apps/sound-app/src-tauri/tauri.conf.json` — asset-protocol config + CSP
  `media-src` addition.
- `apps/sound-app/src-tauri/capabilities/default.json` — add
  `opener:allow-reveal-item-in-dir`.
- `apps/sound-app/package.json` — add `@tauri-apps/plugin-opener`.
- `apps/sound-app/src/App.tsx` — add the `view` toggle and conditional render.

## Key Implementation Details (validated against the real crate/package)

- **The `protocol-asset` Cargo feature is mandatory, not optional.** Enabling
  `security.assetProtocol` in `tauri.conf.json` without also adding
  `features = ["protocol-asset"]` to the `tauri` dependency in `Cargo.toml`
  fails `cargo build` with: *"The `tauri` dependency features on the
  `Cargo.toml` file does not match the allowlist defined under
  `tauri.conf.json`. Please run `tauri dev` or `tauri build` or add the
  `protocol-asset` feature."* This was discovered by actually running
  `cargo build` against the real crate during plan validation — no prior PR
  in this migration needed this feature flag, so it's easy to miss.
- **`$AUDIO` is a real Tauri path variable**, confirmed in the installed
  `tauri` crate source (`BaseDirectory::Audio => "$AUDIO"`), matching
  `writer::recording_dir`'s use of the OS audio directory + `"Sound
  Recorder"` subdirectory. The asset-protocol scope glob is
  `"$AUDIO/Sound Recorder/**"`.
- **CSP needs a `media-src` addition** — the existing
  `default-src 'self'; style-src 'self' 'unsafe-inline'` blocks `<audio>`
  from loading an `asset:`-protocol URL under Tauri's strict default. Add
  `media-src 'self' asset: http://asset.localhost`.
- **`convertFileSrc(path)` from `@tauri-apps/api/core`** (already a
  dependency, confirmed by reading the compiled JS, not just its `.d.ts`)
  converts an absolute filesystem path to a browser-loadable
  `asset://`/`http://asset.localhost` URL. This is what feeds `<audio src>`.
- **`@tauri-apps/plugin-opener`'s `revealItemInDir(path: string | string[]):
  Promise<void>`** is the confirmed real signature (JS package v2.5.5,
  matching Rust crate `tauri-plugin-opener` v2.5.5). Registered in `lib.rs`
  via `.plugin(tauri_plugin_opener::init())`. The exact capability
  permission identifier, confirmed via the crate's own
  `permissions/autogenerated/commands/reveal_item_in_dir.toml`, is
  `opener:allow-reveal-item-in-dir` — nothing broader is needed.
- **`hound::WavReader::open(path)`** gives `.spec()` (sample rate, channels,
  bits) and `.duration()` (per-channel frame count — NOT interleaved sample
  count). Duration in ms is
  `(duration_frames as u64 * 1000) / spec.sample_rate.max(1) as u64`.
- **Real `Button` component props** (confirmed by reading
  `packages/ui/src/components/button.tsx`): `variant` is one of
  `default`/`outline`/`secondary`/`ghost`/`destructive`/`link`; `size` is one
  of `default`/`xs`/`sm`/`lg`/`icon`/`icon-xs`/`icon-sm`/`icon-lg`.
- **A known, non-blocking lint warning**: `RecordingsList.tsx`'s
  fetch-on-mount `useEffect(() => { void refresh() }, [])` triggers
  `eslint-plugin-react-hooks`' `set-state-in-effect` rule (installed version
  7.1.1, part of the shared `react-internal` config's "recommended" rules)
  as a **warning**, not an error — `pnpm --filter sound-app lint` still
  exits 0. This is a standard, unavoidable fetch-on-mount pattern with no
  existing repo precedent to follow instead (verified: no other component in
  this codebase does client-side data fetching on mount — `useRecordingState`
  only subscribes to Tauri events, it never fetches). Do not attempt to
  "fix" this warning by suppressing it or restructuring the effect; it is
  expected and was confirmed via a real `pnpm --filter sound-app lint` run
  against this exact code during plan validation.

---

## Task 1: `recordings.rs` — list/rename/delete logic, fully tested

**Files:**
- Create: `apps/sound-app/src-tauri/src/recordings.rs`
- Modify: `apps/sound-app/src-tauri/src/lib.rs:1-8` (add `mod recordings;`
  alphabetically before `mod recovery;`)

**Interfaces:**
- Produces: `pub struct RecordingMeta { path: String, filename: String,
  created_at_ms: u64, duration_ms: u64, size_bytes: u64 }` (Serialize);
  `pub fn list_recordings(dir: &Path) -> std::io::Result<Vec<RecordingMeta>>`;
  `pub fn rename_recording(dir: &Path, old_name: &str, new_name: &str) ->
  Result<String, String>`; `pub fn delete_recording(dir: &Path, name: &str)
  -> Result<(), String>`. Task 2's `commands.rs` wrappers consume all four.

- [ ] **Step 1: Create `apps/sound-app/src-tauri/src/recordings.rs` with the full module below**

```rust
use std::path::Path;

#[derive(serde::Serialize)]
pub struct RecordingMeta {
  pub path: String,
  pub filename: String,
  pub created_at_ms: u64,
  pub duration_ms: u64,
  pub size_bytes: u64,
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

  Some(RecordingMeta {
    path: path.to_string_lossy().to_string(),
    filename: path.file_name()?.to_string_lossy().to_string(),
    created_at_ms,
    duration_ms,
    size_bytes,
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
  recordings.sort_by_key(|r| std::cmp::Reverse(r.created_at_ms));
  Ok(recordings)
}

/// Rejects a `new_name` containing a path separator, not ending in `.wav`,
/// or colliding with an existing file. On success, renames
/// `dir/old_name` to `dir/new_name` and returns the new full path.
pub fn rename_recording(dir: &Path, old_name: &str, new_name: &str) -> Result<String, String> {
  if new_name.contains('/') || new_name.contains('\\') {
    return Err("New name cannot contain a path separator".to_string());
  }
  if !new_name.ends_with(".wav") {
    return Err("New name must end in .wav".to_string());
  }
  let old_path = dir.join(old_name);
  let new_path = dir.join(new_name);
  if new_path.exists() {
    return Err("A recording with that name already exists".to_string());
  }
  std::fs::rename(&old_path, &new_path).map_err(|e| format!("Could not rename: {e}"))?;
  Ok(new_path.to_string_lossy().to_string())
}

/// Rejects a `name` containing a path separator (defense-in-depth against a
/// malformed path escaping the save directory), then deletes `dir/name`.
pub fn delete_recording(dir: &Path, name: &str) -> Result<(), String> {
  if name.contains('/') || name.contains('\\') {
    return Err("Name cannot contain a path separator".to_string());
  }
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
```

- [ ] **Step 2: Register the module in `lib.rs`**

Add `mod recordings;` to `apps/sound-app/src-tauri/src/lib.rs`, alphabetically
placed before `mod recovery;`:

```rust
mod capture;
mod commands;
mod recordings;
mod recovery;
mod state;
mod storage;
mod tick;
mod writer;
```

- [ ] **Step 3: Run the new tests**

Run: `cargo test recordings:: --manifest-path apps/sound-app/src-tauri/Cargo.toml`
Expected: PASS, 11 tests (`list_recordings_reads_duration_from_the_wav_header`,
`list_recordings_excludes_wav_tmp_files`,
`list_recordings_recognizes_any_wav_file_not_just_the_recording_prefix`,
`list_recordings_sorts_newest_first`,
`list_recordings_skips_a_corrupt_file_instead_of_aborting_the_scan`,
`rename_recording_succeeds_and_renames_the_file`,
`rename_recording_rejects_a_path_separator`,
`rename_recording_rejects_a_non_wav_extension`,
`rename_recording_rejects_a_collision`,
`delete_recording_removes_the_file`,
`delete_recording_rejects_a_path_separator`).

- [ ] **Step 4: Run clippy and fmt**

Run: `cargo clippy --all-targets --manifest-path apps/sound-app/src-tauri/Cargo.toml`
and `cargo fmt --check --manifest-path apps/sound-app/src-tauri/Cargo.toml`
Expected: both clean, 0 warnings.

Note: if clippy flags `sort_by` with a `Reverse` comparator as
`clippy::unnecessary_sort_by`, the code above already uses
`sort_by_key(|r| std::cmp::Reverse(r.created_at_ms))`, which is clean — do
not reintroduce a raw `sort_by` closure.

- [ ] **Step 5: Commit**

```bash
git add apps/sound-app/src-tauri/src/recordings.rs apps/sound-app/src-tauri/src/lib.rs
git commit -m "feat(sound-app): add recordings.rs for listing/renaming/deleting saved WAV files"
```

---

## Task 2: `commands.rs` wrappers + Tauri registration

**Files:**
- Modify: `apps/sound-app/src-tauri/src/commands.rs` (add imports + three
  commands after `cancel_recording`)
- Modify: `apps/sound-app/src-tauri/src/lib.rs` (register the three commands
  in `tauri::generate_handler!`)

**Interfaces:**
- Consumes: `recordings::list_recordings`/`rename_recording`/
  `delete_recording` from Task 1; `writer::recording_dir(&AppHandle) ->
  Result<PathBuf, String>` (existing, already used elsewhere in
  `commands.rs`); `state::CommandError::new(String) -> CommandError`
  (existing).
- Produces: `#[tauri::command] list_recordings(app: AppHandle) ->
  Result<Vec<RecordingMeta>, CommandError>`; `#[tauri::command]
  rename_recording(app: AppHandle, old_name: String, new_name: String) ->
  Result<String, CommandError>`; `#[tauri::command] delete_recording(app:
  AppHandle, name: String) -> Result<(), CommandError>`. The frontend
  (Task 5) calls these via `invoke("list_recordings")`,
  `invoke("rename_recording", { oldName, newName })`,
  `invoke("delete_recording", { name })` — Tauri auto-converts the Rust
  snake_case parameter names to camelCase on the JS side.

- [ ] **Step 1: Add the import**

In `apps/sound-app/src-tauri/src/commands.rs`, add this line to the existing
import block (near the top, alongside the other `use crate::...` lines):

```rust
use crate::recordings::{self, RecordingMeta};
```

- [ ] **Step 2: Add the three command wrappers**

Append after the existing `cancel_recording` function in `commands.rs`:

```rust
#[tauri::command]
pub fn list_recordings(app: AppHandle) -> Result<Vec<RecordingMeta>, CommandError> {
  let dir = writer::recording_dir(&app).map_err(CommandError::new)?;
  recordings::list_recordings(&dir).map_err(|e| CommandError::new(e.to_string()))
}

#[tauri::command]
pub fn rename_recording(
  app: AppHandle,
  old_name: String,
  new_name: String,
) -> Result<String, CommandError> {
  let dir = writer::recording_dir(&app).map_err(CommandError::new)?;
  recordings::rename_recording(&dir, &old_name, &new_name).map_err(CommandError::new)
}

#[tauri::command]
pub fn delete_recording(app: AppHandle, name: String) -> Result<(), CommandError> {
  let dir = writer::recording_dir(&app).map_err(CommandError::new)?;
  recordings::delete_recording(&dir, &name).map_err(CommandError::new)
}
```

- [ ] **Step 3: Register the commands in `lib.rs`**

In `apps/sound-app/src-tauri/src/lib.rs`, add the three commands to the
`tauri::generate_handler!` list, after `commands::cancel_recording`:

```rust
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
    ])
```

- [ ] **Step 4: Build and test**

Run: `cargo build --manifest-path apps/sound-app/src-tauri/Cargo.toml`
Expected: builds clean (no `protocol-asset` feature is needed yet at this
point — that's Task 3 — but this task's code doesn't touch the asset
protocol, so it should already build against the crate as-is).

Run: `cargo test --manifest-path apps/sound-app/src-tauri/Cargo.toml`
Expected: PASS, all existing tests plus the 11 from Task 1 (51 total as of
this plan's validation — the exact count may drift slightly if earlier PRs
add tests before this one lands, but it must not decrease).

- [ ] **Step 5: Commit**

```bash
git add apps/sound-app/src-tauri/src/commands.rs apps/sound-app/src-tauri/src/lib.rs
git commit -m "feat(sound-app): add list/rename/delete_recording Tauri commands"
```

---

## Task 3: Asset protocol + opener plugin wiring

**Files:**
- Modify: `apps/sound-app/src-tauri/Cargo.toml`
- Modify: `apps/sound-app/src-tauri/tauri.conf.json`
- Modify: `apps/sound-app/src-tauri/capabilities/default.json`
- Modify: `apps/sound-app/src-tauri/src/lib.rs`
- Modify: `apps/sound-app/package.json` (adds `@tauri-apps/plugin-opener`)

**Interfaces:**
- Produces: the `asset://`/`http://asset.localhost` URL scheme becomes
  loadable from the frontend for any path under `$AUDIO/Sound Recorder/**`;
  the Rust command `revealItemInDir` (via the opener plugin, not a command
  this codebase defines) becomes callable from JS. Task 5's frontend
  consumes both.

- [ ] **Step 1: Add the opener plugin dependency and the `protocol-asset` feature**

Modify `apps/sound-app/src-tauri/Cargo.toml`'s dependency block. The existing
`tauri` line:

```toml
tauri = { version = "2.11.6", features = [] }
```

becomes:

```toml
tauri = { version = "2.11.6", features = ["protocol-asset"] }
```

And add a new line anywhere in the `[dependencies]` section (alphabetical
placement after `tauri-plugin-log` keeps the existing ordering style):

```toml
tauri-plugin-opener = "2.5.5"
```

Run: `cargo build --manifest-path apps/sound-app/src-tauri/Cargo.toml`
Expected: PASS. If you skip the `protocol-asset` feature and only add the
`assetProtocol` config in Step 2 below, this build fails with: *"The `tauri`
dependency features on the `Cargo.toml` file does not match the allowlist
defined under `tauri.conf.json`... add the `protocol-asset` feature."* —
this was verified against the real crate during plan validation.

- [ ] **Step 2: Configure the asset protocol and CSP in `tauri.conf.json`**

Find the existing `"security"` block in
`apps/sound-app/src-tauri/tauri.conf.json`:

```json
    "security": {
      "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'"
    }
```

Replace it with:

```json
    "security": {
      "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'; media-src 'self' asset: http://asset.localhost",
      "assetProtocol": {
        "enable": true,
        "scope": ["$AUDIO/Sound Recorder/**"]
      }
    }
```

Note: `"Sound Recorder"` matches `writer::recording_dir`'s existing
subdirectory name under the OS audio directory exactly — do not introduce a
different casing/spacing here, or the scope glob won't match real recording
paths.

- [ ] **Step 3: Grant the reveal-in-file-manager capability**

In `apps/sound-app/src-tauri/capabilities/default.json`, the existing
`"permissions"` array:

```json
  "permissions": [
    "core:default"
  ]
```

becomes:

```json
  "permissions": [
    "core:default",
    "opener:allow-reveal-item-in-dir"
  ]
```

- [ ] **Step 4: Register the opener plugin in `lib.rs`**

In `apps/sound-app/src-tauri/src/lib.rs`, add `.plugin(tauri_plugin_opener::init())`
to the builder chain, before `.manage(...)`:

```rust
    .plugin(tauri_plugin_opener::init())
    .manage(SharedState::new(Box::new(LinuxPulseCapture::new())))
```

- [ ] **Step 5: Add the JS package**

```bash
pnpm --filter sound-app add @tauri-apps/plugin-opener
```

Expected: adds `"@tauri-apps/plugin-opener": "^2.5.5"` to
`apps/sound-app/package.json`'s dependencies (matching the Rust crate
version) and updates `pnpm-lock.yaml`.

- [ ] **Step 6: Build and verify**

Run: `cargo build --manifest-path apps/sound-app/src-tauri/Cargo.toml`
Expected: PASS.

Run: `cargo test --manifest-path apps/sound-app/src-tauri/Cargo.toml`
Expected: PASS, same test count as end of Task 2.

- [ ] **Step 7: Commit**

```bash
git add apps/sound-app/src-tauri/Cargo.toml apps/sound-app/src-tauri/Cargo.lock \
  apps/sound-app/src-tauri/tauri.conf.json apps/sound-app/src-tauri/capabilities/default.json \
  apps/sound-app/src-tauri/src/lib.rs apps/sound-app/package.json pnpm-lock.yaml
git commit -m "feat(sound-app): enable asset protocol for recording playback and opener plugin for reveal-in-file-manager"
```

---

## Task 4: `RecordingsList.tsx` frontend component

**Files:**
- Create: `apps/sound-app/src/components/RecordingsList.tsx`
- Create: `apps/sound-app/src/components/RecordingsList.test.tsx`

**Interfaces:**
- Consumes: `invoke` and `convertFileSrc` from `@tauri-apps/api/core`;
  `revealItemInDir` from `@tauri-apps/plugin-opener` (Task 3); the Tauri
  commands `list_recordings`/`rename_recording`/`delete_recording` (Task 2),
  called as `invoke<RecordingMeta[]>("list_recordings")`,
  `invoke("rename_recording", { oldName, newName })`,
  `invoke("delete_recording", { name })`; `Button` from
  `@workspace/ui/components/button` (existing).
- Produces: `export type RecordingMeta = { path: string; filename: string;
  created_at_ms: number; duration_ms: number; size_bytes: number }`;
  `export function RecordingsList(): JSX.Element`. Task 5's `App.tsx`
  imports `RecordingsList` (no props).

- [ ] **Step 1: Write the failing test file**

Create `apps/sound-app/src/components/RecordingsList.test.tsx`:

```tsx
import { render, screen, waitFor } from "@testing-library/react"
import { fireEvent } from "@testing-library/react"
import { beforeEach, describe, expect, it, vi } from "vitest"

import { invoke } from "@tauri-apps/api/core"

import { RecordingsList } from "./RecordingsList"

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  convertFileSrc: (path: string) => `asset://localhost/${path}`,
}))

vi.mock("@tauri-apps/plugin-opener", () => ({
  revealItemInDir: vi.fn(),
}))

const mockedInvoke = invoke as unknown as ReturnType<typeof vi.fn>

const sampleRecording = {
  path: "/home/user/Music/Sound Recorder/recording-a.wav",
  filename: "recording-a.wav",
  created_at_ms: 1_700_000_000_000,
  duration_ms: 65_000,
  size_bytes: 2 * 1024 * 1024,
}

describe("RecordingsList", () => {
  beforeEach(() => {
    mockedInvoke.mockReset()
  })

  it("renders a fetched recording with its duration and size", async () => {
    mockedInvoke.mockResolvedValueOnce([sampleRecording])

    render(<RecordingsList />)

    await waitFor(() => {
      expect(screen.getByText("recording-a.wav")).toBeInTheDocument()
    })
    expect(screen.getByText(/01:05/)).toBeInTheDocument()
    expect(screen.getByText(/2\.0 MB/)).toBeInTheDocument()
  })

  it("shows an empty state when there are no recordings", async () => {
    mockedInvoke.mockResolvedValueOnce([])

    render(<RecordingsList />)

    await waitFor(() => {
      expect(screen.getByText("No recordings yet.")).toBeInTheDocument()
    })
  })

  it("calls delete_recording after confirming, then refreshes the list", async () => {
    mockedInvoke.mockResolvedValueOnce([sampleRecording])
    mockedInvoke.mockResolvedValueOnce(undefined)
    mockedInvoke.mockResolvedValueOnce([])
    vi.spyOn(window, "confirm").mockReturnValue(true)

    render(<RecordingsList />)
    await waitFor(() => {
      expect(screen.getByText("recording-a.wav")).toBeInTheDocument()
    })

    fireEvent.click(screen.getByRole("button", { name: "Delete" }))

    await waitFor(() => {
      expect(mockedInvoke).toHaveBeenCalledWith("delete_recording", {
        name: "recording-a.wav",
      })
    })
    await waitFor(() => {
      expect(screen.getByText("No recordings yet.")).toBeInTheDocument()
    })
  })

  it("does not call delete_recording when the confirmation is declined", async () => {
    mockedInvoke.mockResolvedValueOnce([sampleRecording])
    vi.spyOn(window, "confirm").mockReturnValue(false)

    render(<RecordingsList />)
    await waitFor(() => {
      expect(screen.getByText("recording-a.wav")).toBeInTheDocument()
    })

    fireEvent.click(screen.getByRole("button", { name: "Delete" }))

    expect(mockedInvoke).not.toHaveBeenCalledWith(
      "delete_recording",
      expect.anything()
    )
  })
})
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `pnpm --filter sound-app test RecordingsList`
Expected: FAIL — `./RecordingsList` does not exist yet.

- [ ] **Step 3: Create the component**

Create `apps/sound-app/src/components/RecordingsList.tsx`:

```tsx
import { useEffect, useState } from "react"

import { convertFileSrc, invoke } from "@tauri-apps/api/core"
import { revealItemInDir } from "@tauri-apps/plugin-opener"

import { Button } from "@workspace/ui/components/button"

export type RecordingMeta = {
  path: string
  filename: string
  created_at_ms: number
  duration_ms: number
  size_bytes: number
}

type CommandError = { message: string; recoverable: boolean }

function errorMessage(err: unknown): string {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as CommandError).message)
  }
  return String(err)
}

function formatDuration(ms: number): string {
  const totalSeconds = Math.floor(ms / 1000)
  const minutes = Math.floor(totalSeconds / 60)
  const seconds = totalSeconds % 60
  return `${minutes.toString().padStart(2, "0")}:${seconds.toString().padStart(2, "0")}`
}

function formatSize(bytes: number): string {
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(0)} KB`
  }
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

function formatDate(ms: number): string {
  return new Date(ms).toLocaleString()
}

export function RecordingsList() {
  const [recordings, setRecordings] = useState<RecordingMeta[]>([])
  const [error, setError] = useState<string | null>(null)
  const [renamingPath, setRenamingPath] = useState<string | null>(null)
  const [renameValue, setRenameValue] = useState("")

  async function refresh() {
    try {
      const result = await invoke<RecordingMeta[]>("list_recordings")
      setRecordings(result)
      setError(null)
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  useEffect(() => {
    void refresh()
  }, [])

  function startRename(recording: RecordingMeta) {
    setRenamingPath(recording.path)
    setRenameValue(recording.filename)
  }

  async function confirmRename(recording: RecordingMeta) {
    try {
      await invoke("rename_recording", {
        oldName: recording.filename,
        newName: renameValue,
      })
      setRenamingPath(null)
      await refresh()
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  async function handleDelete(recording: RecordingMeta) {
    if (!window.confirm(`Delete "${recording.filename}"?`)) {
      return
    }
    try {
      await invoke("delete_recording", { name: recording.filename })
      await refresh()
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  async function handleReveal(recording: RecordingMeta) {
    try {
      await revealItemInDir(recording.path)
    } catch (err) {
      setError(errorMessage(err))
    }
  }

  return (
    <div className="flex flex-col gap-4">
      {error && (
        <div className="rounded border border-destructive p-2 text-sm text-destructive">
          {error}
        </div>
      )}

      {recordings.length === 0 && (
        <p className="text-sm text-muted-foreground">No recordings yet.</p>
      )}

      <ul className="flex flex-col gap-3">
        {recordings.map((recording) => (
          <li
            key={recording.path}
            className="flex flex-col gap-2 rounded border p-3"
          >
            <div className="flex items-center justify-between gap-2">
              {renamingPath === recording.path ? (
                <input
                  className="w-full rounded border p-1 text-sm"
                  value={renameValue}
                  onChange={(e) => setRenameValue(e.target.value)}
                  autoFocus
                />
              ) : (
                <span className="font-medium">{recording.filename}</span>
              )}
              <span className="text-xs text-muted-foreground">
                {formatDate(recording.created_at_ms)}
              </span>
            </div>

            <audio
              controls
              src={convertFileSrc(recording.path)}
              className="w-full"
            />

            <div className="flex items-center justify-between gap-2 text-xs text-muted-foreground">
              <span>
                {formatDuration(recording.duration_ms)} &middot;{" "}
                {formatSize(recording.size_bytes)}
              </span>
              <div className="flex gap-2">
                {renamingPath === recording.path ? (
                  <>
                    <Button
                      size="sm"
                      onClick={() => void confirmRename(recording)}
                    >
                      Save
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => setRenamingPath(null)}
                    >
                      Cancel
                    </Button>
                  </>
                ) : (
                  <>
                    <Button size="sm" onClick={() => startRename(recording)}>
                      Rename
                    </Button>
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => void handleReveal(recording)}
                    >
                      Reveal
                    </Button>
                    <Button
                      size="sm"
                      variant="destructive"
                      onClick={() => void handleDelete(recording)}
                    >
                      Delete
                    </Button>
                  </>
                )}
              </div>
            </div>
          </li>
        ))}
      </ul>
    </div>
  )
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `pnpm --filter sound-app test RecordingsList`
Expected: PASS, 4 tests.

- [ ] **Step 5: Typecheck and lint**

Run: `pnpm --filter sound-app typecheck`
Expected: PASS, no errors.

Run: `pnpm --filter sound-app lint`
Expected: exits 0. One expected warning on the `useEffect(() => { void
refresh() }, [])` line (`react-hooks/set-state-in-effect`) — see "Key
Implementation Details" above. This is a warning, not an error; do not
treat it as a task failure.

- [ ] **Step 6: Commit**

```bash
git add apps/sound-app/src/components/RecordingsList.tsx apps/sound-app/src/components/RecordingsList.test.tsx
git commit -m "feat(sound-app): add RecordingsList component with play/rename/reveal/delete"
```

---

## Task 5: `App.tsx` view toggle

**Files:**
- Modify: `apps/sound-app/src/App.tsx`
- Modify: `apps/sound-app/src/App.test.tsx` (if it exists and needs a new
  assertion — see Step 3)

**Interfaces:**
- Consumes: `RecordingsList` from `./components/RecordingsList` (Task 4, no
  props).

- [ ] **Step 1: Add the `view` state and import**

In `apps/sound-app/src/App.tsx`, add the import:

```tsx
import { RecordingsList } from "./components/RecordingsList"
```

and inside `App()`, alongside the existing `sourceOverride` state:

```tsx
const [view, setView] = useState<"recorder" | "recordings">("recorder")
```

- [ ] **Step 2: Add the toggle button and conditional render**

Replace the existing return statement's top-level structure. The full
`return` block becomes:

```tsx
return (
  <ThemeProvider>
    <div className="flex min-h-svh flex-col gap-4 p-6">
      <div className="flex items-center justify-between">
        <h1 className="font-medium">Sound Recorder</h1>
        <Button
          variant="outline"
          size="sm"
          onClick={() =>
            setView(view === "recorder" ? "recordings" : "recorder")
          }
        >
          {view === "recorder" ? "Recordings" : "Back to Recorder"}
        </Button>
      </div>

      {view === "recordings" ? (
        <RecordingsList />
      ) : (
        <>
          {error && (
            <div className="rounded border border-destructive p-2 text-sm text-destructive">
              {error}
            </div>
          )}

          {canStart && sources.length > 0 && (
            <select
              className="w-fit rounded border p-2 text-sm"
              value={selectedSourceId}
              onChange={(e) => setSourceOverride(e.target.value)}
            >
              {sources.map((source) => (
                <option key={source.id} value={source.id}>
                  {source.name}
                </option>
              ))}
            </select>
          )}

          <div className="font-mono text-2xl">
            {formatElapsed(isActive ? elapsedMs : 0)}
          </div>

          {isActive && (
            <div className="h-2 w-full max-w-xs rounded bg-muted">
              <div
                className="h-2 rounded bg-primary transition-all"
                style={{ width: `${Math.min(level, 1) * 100}%` }}
              />
            </div>
          )}

          {state.state === "Saving" && (
            <div className="text-sm text-muted-foreground">Saving…</div>
          )}

          <div className="flex gap-2">
            {canStart && sources.length > 0 && (
              <Button onClick={() => void startRecording(selectedSourceId)}>
                Record
              </Button>
            )}
            {isRecording && (
              <Button onClick={() => void pauseRecording()}>Pause</Button>
            )}
            {isPaused && (
              <Button onClick={() => void resumeRecording()}>Resume</Button>
            )}
            {isActive && (
              <Button onClick={() => void stopRecording()}>Stop</Button>
            )}
            {isActive && <Button onClick={handleCancel}>Cancel</Button>}
          </div>
        </>
      )}
    </div>
  </ThemeProvider>
)
```

Everything inside the recorder branch (error banner, source select, elapsed
timer, level meter, Saving indicator, Record/Pause/Resume/Stop/Cancel
buttons) is unchanged from the pre-existing PR6 code — only the wrapping
`view === "recordings" ? ... : (...)` conditional and the header toggle
button are new.

- [ ] **Step 3: Check `App.test.tsx` still passes**

Read `apps/sound-app/src/App.test.tsx`. If its existing tests render `<App
/>` and assert against the recorder view's content (the default `view`
state is `"recorder"`, so existing assertions should be unaffected), no
change is needed. If it mocks `@tauri-apps/api/core`'s `invoke` with a
`mockImplementation` that only handles recording-state commands (e.g.
`list_sources`, `start_recording`), verify it either also stubs
`list_recordings` (since `RecordingsList` is not rendered in the default
`"recorder"` view, `list_recordings` is never called and no stub is needed)
— confirm this by reading the mock's structure before assuming no change is
required.

Run: `pnpm --filter sound-app test`
Expected: PASS, no regressions in `App.test.tsx`.

- [ ] **Step 4: Typecheck and lint**

Run: `pnpm --filter sound-app typecheck`
Expected: PASS.

Run: `pnpm --filter sound-app lint`
Expected: exits 0 (the one expected `RecordingsList.tsx` warning from Task 4
persists; `App.tsx` itself introduces no new warnings).

- [ ] **Step 5: Manual verification (UI change — requires a real display)**

Run: `pnpm --filter sound-app tauri dev`. In the running app:
1. Record something briefly, stop it.
2. Click "Recordings" — confirm the just-saved recording appears with a
   correct date, duration, and size.
3. Click the row's `<audio>` player's play control — confirm real audio
   plays back.
4. Click "Rename", change the name, click "Save" — confirm the row updates
   and the file is renamed on disk.
5. Click "Reveal" — confirm the OS file manager opens showing the file.
6. Click "Delete" — confirm the `window.confirm` dialog appears; declining
   leaves the file, confirming removes it from the list and from disk.
7. Click "Back to Recorder" — confirm the recorder view returns to its
   prior state (not reset).

This must be checked in the actual running app, not just via passing tests.

- [ ] **Step 6: Commit**

```bash
git add apps/sound-app/src/App.tsx apps/sound-app/src/App.test.tsx
git commit -m "feat(sound-app): add recordings view toggle to App"
```

---

## Verification

```bash
# from repo root
cargo test --manifest-path apps/sound-app/src-tauri/Cargo.toml
cargo clippy --all-targets --manifest-path apps/sound-app/src-tauri/Cargo.toml
cargo fmt --check --manifest-path apps/sound-app/src-tauri/Cargo.toml
pnpm --filter sound-app test
pnpm --filter sound-app typecheck
pnpm --filter sound-app lint          # exits 0; one expected RecordingsList.tsx warning
pnpm --filter sound-app tauri dev     # manual: full record -> list -> play -> rename -> reveal -> delete flow
git status                            # confirm no leftover validation-phase artifacts
```

All automated commands must exit 0 (the one lint warning documented above is
expected and does not fail the `lint` command itself). The manual `tauri
dev` walkthrough in Task 5 Step 5 is required — passing tests alone don't
confirm real playback, real file rename/delete, or real OS file-manager
integration actually work.
