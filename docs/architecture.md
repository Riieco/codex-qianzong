# Architecture

## Read When

- Before changing Tauri commands, Rust data services, desktop shell behavior, or cross-platform boundaries.

## Owner

- Desktop / Architecture

## Update Trigger

- IPC commands, native capabilities, data source behavior, packaging, or platform support changes.

## Validation

- `npm run build`, `npm run rust:check`, and relevant Rust/frontend tests pass.

## Overview

`codex-qianzong` is an independent Tauri 2 application under the original `codexU` repository. It does not extend the old Swift app. The old Swift implementation remains reference material for data semantics and UI information architecture.

## Layers

| Layer          | Owns                                                                                     | Must Not Own                                       |
| -------------- | ---------------------------------------------------------------------------------------- | -------------------------------------------------- |
| React UI       | Layout, visual state, loading/empty/error states, settings form                          | Local filesystem reads, process execution, secrets |
| Frontend API   | Stable `invoke` wrappers and browser mock fallback                                       | Business logic or privileged native behavior       |
| Tauri Commands | IPC boundary, typed request/response, command validation                                 | UI rendering                                       |
| Rust Services  | Codex app-server, SQLite, JSONL, auth vault, history migration, settings, path detection | View-specific formatting                           |
| Desktop Shell  | Tray, global shortcut, window visibility/topmost behavior                                | Data parsing logic                                 |

## Native Boundary

Stable commands:

- `get_usage_snapshot`
- `refresh_task_board`
- `get_app_settings`
- `save_app_settings`
- `get_auth_credential_status`
- `clear_relay_api_key`
- `has_unified_history_backup`
- `restore_unified_history`
- `list_codex_config_backups`
- `create_codex_config_backup`
- `restore_codex_config_backup`
- `delete_codex_config_backup`
- `get_detection_paths`
- `open_log_folder`

Command return values are serializable Rust structs that mirror the TypeScript types in `src/types/usage.ts`.

## Platform Strategy

- Windows and macOS are first-class targets.
- Windows uses tray + topmost window rather than exact desktop-layer attachment. The main window is locked to `930x760` at configuration and runtime levels; resize, maximize, and fullscreen transitions are disabled.
- macOS keeps `Command+U` parity with the original app.
- Codex executable and data paths are auto-detected but can be overridden in settings.
- Account-level 7-day/30-day trends prefer Codex app-server `account/usage/read`.
- Token value cards and membership-period value progress prefer local JSONL `token_count` parsing because it exposes uncached input, cached input, and output token splits for official API-price estimation. Official aggregate usage is only a fallback for value when JSONL details are unavailable.
- Access mode settings control UI state, dashboard data-source selection, and Codex `config.toml` synchronization. Official native mode keeps official account/app-server reads; API relay mode uses local SQLite/JSONL statistics.
- Optional unified history gives official and relay modes the same managed provider ID (`qianzong_unified`) so Codex can resume sessions created in either mode. Existing JSONL metadata and SQLite thread rows are migrated only when the user explicitly enables migration.

## Security Notes

- Usage aggregation opens SQLite read-only. Explicit unified-history migration opens Codex state databases read/write with a busy timeout, backup API snapshot, and transaction.
- Shell execution is limited to `codex app-server` and opening the app log folder.
- The UI receives diagnostics and sanitized status, not raw secrets.
- Global shortcut permissions are declared in Tauri capabilities.
- Borderless window dragging, minimize, and close use the Tauri window API and require explicit `core:window:*` permissions in `src-tauri/capabilities/default.json`.
- API relay settings store endpoint, model, reasoning effort, and speed in plain settings. API keys never enter `settings.json` or frontend responses.
- `src-tauri/src/auth_vault.rs` stores the relay endpoint/API Key and the complete official `auth.json` snapshot in a versioned ChaCha20-Poly1305 vault. The random master key is stored in the operating-system keyring. The relay Key is bound to its endpoint; changing the endpoint requires a new Key.
- Switching to relay writes only relay auth fields to Codex `auth.json`. Switching back restores the full saved official auth object, including access/refresh/ID tokens and fields unknown to this app.
- Codex config/auth, credential-vault, and JSONL rewrites use same-directory temporary files and atomic replacement. Windows uses `MoveFileExW` with replace and write-through flags.
- Unified-history migration keeps timestamped JSONL and SQLite backups plus a per-session/thread source-provider ledger. Restore is available only after unified mode is disabled and changes only records still owned by `qianzong_unified`.
- Saving settings rewrites the user Codex config only through `src-tauri/src/codex_config.rs`, with a first-run default snapshot, restore snapshot, and timestamped backups. The settings UI can list, create, and restore named backup metadata, but it never receives raw config or auth contents.
