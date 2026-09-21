# sound-app: Recordings List Screen (PR 7 of 8)

Status: approved for planning
Scope: `apps/sound-app` only. Depends on PR 6 (storage checks + error surfacing, merged to master).

## Context

The app can now record real audio to real, atomically-finalized WAV files
(PR 5), with storage checks and a fully guarded state machine (PR 6) — but
there is still no way to see, play back, rename, or delete a saved
recording once `Saved` fades from view. This PR adds that screen.

Key PRD requirements this PR must honor:
- §5: "As a user, I can find, play, rename, export, and delete saved
  recordings."
- §6.1: "Provide a recordings list with date, duration, format, and file
  size"; "Link to recordings and settings."
- §7 (Recordings Screen): list newest first; support play, rename, delete,
  export/share, open/reveal "where available"; confirm destructive
  deletion.
- Original migration spec's roadmap entry for this PR: "scan save dir, list
  (date/duration/format/size) newest-first; play/rename/delete-with-
  confirmation/reveal-in-file-manager" — and its architecture note: *"No
  database: the filesystem is the source of truth. `list_recordings()`
  scans the save directory and reads duration/sample-rate/channels directly
  from each WAV header — no sidecar metadata to drift out of sync."*

### Carryover context from PR 6's final review

PR 6's review flagged that this PR's directory scan must filter out any
`*.wav.tmp` file — a temp file mid-write during an active recording, or one
that slipped past `recovery::recover_orphaned_recordings`'s startup pass,
must never appear in the list.

### Decisions made during design

- **Recording discovery**: scan the save directory for **any** `*.wav`
  file, not just ones matching the writer's own `recording-...` naming
  convention. This is deliberate: once rename ships, a user renaming a file
  to `Interview with Alex.wav` must not cause it to silently disappear from
  the list — that would defeat the point of offering rename at all.
  `*.wav.tmp` files are explicitly excluded (per the carryover above).
- **No database, no sidecar metadata file.** Each recording's metadata
  (`created_at`, `duration_ms`, `size_bytes`) is derived on every scan from
  the filesystem (mtime, file size) and the WAV file's own header (read via
  `hound`, the same crate PR 5's writer already depends on) — never
  cached. This matches the original migration spec's explicit architecture
  note and means a rename or delete performed outside the app (e.g. in a
  file manager) can never leave stale metadata behind, since there is none
  to go stale.
- **Playback via Tauri's asset protocol, not a bytes-over-IPC command.**
  `tauri.conf.json` enables the asset protocol with its `scope` restricted
  to exactly the save directory (`<OS audio dir>/Sound Recorder/**`) — a
  narrow, justified capability grant, not a blanket filesystem permission.
  The frontend calls `convertFileSrc(path)` and hands the result straight
  to a plain `<audio>` element, getting real seeking/streaming behavior for
  free. The rejected alternative (a command returning file bytes) would
  load a full recording into memory just to play it back and require
  hand-rolling seek support — unnecessary complexity for something the
  asset protocol already solves correctly.
- **Reveal-in-file-manager via `@tauri-apps/plugin-opener`**, Tauri's
  official, narrowly-scoped plugin for exactly this (open paths/URLs,
  reveal an item in the OS file manager) — not a general shell-execution
  capability. This is called directly from the frontend
  (`revealItemInDir(path)`); no new Rust command is needed for it.
- **"Export/share" is satisfied by reveal-in-file-manager, not built as a
  separate action.** The original roadmap's own PR 7 line already omits
  export/share (only the PRD's broader user-story text mentions it); once a
  file is revealed in the file manager, the user can copy/move/attach it
  themselves through their OS's own tools — there's no cloud/network
  target this local-first app would export *to* anyway (explicitly a
  non-goal). No separate Share action is built.
- **Rename validates the new name and stays a plain filesystem rename.** A
  new `rename_recording(old_path, new_name)` command rejects a `new_name`
  containing a path separator (no moving files outside the save
  directory), requires it still end in `.wav`, and rejects a collision with
  an existing file — then does a plain `std::fs::rename` within the same
  directory.
- **Delete confirms via `window.confirm()`**, reusing the exact pattern
  already established for cancelling an active recording (PR 3) — no new
  dialog component.
- **No router.** A plain `view: "recorder" | "recordings"` state in
  `App.tsx`, switched by a link/button. A full router is unnecessary
  complexity for two screens in a desktop app with no deep-linking need.
- **The list re-fetches on mount / after any rename-delete action /
  whenever the user navigates back to it** — not a live-updating feed.
  Recordings only change as a result of user actions this same screen
  already knows about, so there's no need for filesystem-watching or
  polling.

## Architecture

### Module structure (`apps/sound-app/src-tauri/src/`)

```
recordings.rs  — RecordingMeta, list_recordings/rename_recording/
                 delete_recording command logic (scan + WAV-header read +
                 filename validation)
commands.rs     — thin #[tauri::command] wrappers delegating to recordings.rs,
                  matching this crate's existing pattern (thin commands,
                  logic lives in the domain module)
```

### `recordings.rs`

```rust
#[derive(serde::Serialize)]
pub struct RecordingMeta {
  pub path: String,
  pub filename: String,
  pub created_at_ms: u64,   // Unix epoch ms, from filesystem mtime
  pub duration_ms: u64,     // read from the WAV header via hound
  pub size_bytes: u64,
}

pub fn list_recordings(dir: &std::path::Path) -> std::io::Result<Vec<RecordingMeta>> {
  // scan `dir` for `*.wav` (excluding `*.wav.tmp`), build a RecordingMeta
  // per file (skip any file that errors reading its WAV header rather than
  // aborting the whole scan — one corrupt file shouldn't hide the rest),
  // sort newest-first by created_at_ms.
}

pub fn rename_recording(dir: &std::path::Path, old_name: &str, new_name: &str) -> Result<String, String> {
  // validate new_name: no path separators, ends in ".wav", doesn't collide
  // with an existing file in `dir`; then std::fs::rename(dir.join(old_name), dir.join(new_name)).
}

pub fn delete_recording(dir: &std::path::Path, name: &str) -> std::io::Result<()> {
  // std::fs::remove_file(dir.join(name)), after validating `name` has no
  // path separators (defense-in-depth against a malformed path escaping
  // the save directory).
}
```

`commands.rs` gains three thin `#[tauri::command]` wrappers
(`list_recordings`, `rename_recording`, `delete_recording`) that resolve
`writer::recording_dir(app)` and delegate to the functions above, returning
`Result<_, CommandError>` matching every other command in this file.

### Playback capability (`tauri.conf.json` / `capabilities/`)

Enable the asset protocol scoped to the save directory only:

```json
"app": {
  "security": {
    "assetProtocol": {
      "enable": true,
      "scope": ["$AUDIO/Sound Recorder/**"]
    }
  }
}
```

(`$AUDIO` is Tauri's built-in path variable for the OS audio directory —
the same directory `writer::recording_dir` resolves via
`app.path().audio_dir()`, so the scope and the actual save location stay in
sync without hardcoding a platform-specific path.) No `capabilities/`
permission is needed for the asset protocol itself — it is governed
entirely by `tauri.conf.json`'s `security.assetProtocol.scope` above,
independent of the capabilities/command-permission ACL system. The only
capability permission this feature needs is `opener:allow-reveal-item-in-dir`,
and that's for the Reveal-in-file-manager command, unrelated to playback.

### Frontend

- `App.tsx` gains a `view: "recorder" | "recordings"` state and a link to
  switch between the existing recording UI and a new `RecordingsList`
  component.
- `RecordingsList.tsx`: on mount, calls `invoke("list_recordings")`,
  renders each recording's date (from `created_at_ms`), formatted duration
  (reusing `App.tsx`'s existing `formatElapsed` logic), and size; a
  per-row `<audio controls src={convertFileSrc(path)}>`; Rename (inline
  text input, calls `rename_recording`, re-fetches on success), Delete
  (native `window.confirm()`, calls `delete_recording`, re-fetches on
  success), and Reveal (calls `revealItemInDir(path)` directly, no
  re-fetch needed) buttons per row.

## Testing strategy

- **Rust**: `recordings.rs` unit tests using real short WAV files written
  via `hound` (matching this project's established real-file test style
  from `writer.rs`/`recovery.rs`) — verifying `list_recordings` correctly
  reads duration from the header, correctly excludes `*.wav.tmp`, sorts
  newest-first, and skips (not aborts on) a file that fails to parse;
  `rename_recording`'s filename validation (rejects a path separator,
  rejects a non-`.wav` extension, rejects a collision); `delete_recording`
  actually removes the file.
- **Frontend**: Vitest tests for `RecordingsList` — renders fetched
  recordings, wires Rename/Delete/Reveal to the right `invoke`/plugin
  calls, confirms before deleting (mirroring `App.test.tsx`'s existing
  cancel-confirmation test pattern), re-fetches after a successful
  rename/delete.
- **Manual**: `pnpm --filter sound-app tauri dev` — record something real,
  navigate to the recordings list, confirm it appears with correct
  date/duration/size, play it back (real audio, real seeking), rename it,
  confirm the rename is reflected and the file still plays, reveal it in
  the file manager, then delete it (confirming the dialog) and confirm it's
  gone from both the list and the filesystem.

## Explicitly out of scope for this PR

- A separate Export/Share action — reveal-in-file-manager covers the
  desktop use case.
- Pagination, search, or filtering of the recordings list.
- Any editing of recording content (trimming, effects, etc.).
- A settings-configurable save location or filename template (PR 8).
- Windows/macOS-specific asset-protocol or opener-plugin behavior beyond
  what Tauri's cross-platform APIs already provide.
