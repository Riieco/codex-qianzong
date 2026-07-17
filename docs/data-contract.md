# Data Contract

## Read When

- Before changing Rust models, TypeScript types, IPC commands, or data parsing.

## Owner

- Desktop / Data

## Update Trigger

- Codex schema, app-server method, token pricing, task grouping, or settings shape changes.

## Validation

- Rust and TypeScript builds pass; tests cover affected parsing or rendering behavior.

## Core Types

Rust source of truth: `src-tauri/src/models.rs`

TypeScript mirror: `src/types/usage.ts`

Important models:

- `UsageSnapshot`
- `RateWindow`
- `TokenBreakdown`
- `PricedTokenUsage`
- `DetailedUsage`
- `OfficialUsage`
- `LocalUsage`
- `TaskBoard`
- `TaskItem`
- `DiagnosticItem`
- `AppSettings`

## IPC Commands

| Command                       | Purpose                                                                                              |
| ----------------------------- | ---------------------------------------------------------------------------------------------------- |
| `get_usage_snapshot`          | Full quota, local usage, task board, diagnostics, messages                                           |
| `refresh_task_board`          | Lightweight task board refresh                                                                       |
| `get_app_settings`            | Read persisted app settings                                                                          |
| `save_app_settings`           | Persist settings, sync Codex config/auth, and schedule requested history migration                   |
| `get_auth_credential_status`  | Return booleans for saved official auth/relay Key and the bound relay endpoint; never return secrets |
| `clear_relay_api_key`         | Remove the encrypted relay credential while preserving saved official auth                           |
| `fetch_api_models`            | Fetch filtered OpenAI model IDs from the configured `GET /v1/models` endpoint                        |
| `has_unified_history_backup`  | Report whether a migration ledger exists for the active Codex data directory                         |
| `restore_unified_history`     | Restore migrated JSONL/SQLite provider IDs after unified history is disabled                         |
| `list_codex_config_backups`   | Return metadata for managed Codex config/auth backup snapshots                                       |
| `create_codex_config_backup`  | Save the current Codex `config.toml` and `auth.json` snapshot, returning the refreshed backup list   |
| `restore_codex_config_backup` | Restore a selected managed snapshot after timestamp-backing up the current Codex files               |
| `delete_codex_config_backup`  | Delete a selected non-default managed backup directory and return the refreshed backup list          |
| `get_detection_paths`         | Return detected Codex executable, data dir, DB, and log dir                                          |
| `open_log_folder`             | Open app log folder using OS shell                                                                   |
| `get_skill_board`             | Return local Codex Skills metadata for the isolated Skills board                                     |
| `disable_skill`               | Move an allowed user skill to the local `skills-disabled` folder                                     |
| `enable_skill`                | Move a disabled skill from local `skills-disabled` back to `skills`                                  |
| `archive_skill`               | Move an allowed user skill to the local `skills-trash` folder                                        |
| `open_skill_folder`           | Open a resolved skill folder using the OS file manager                                               |

## Data Semantics

- `get_usage_snapshot` must run snapshot aggregation on a blocking worker thread so app-server startup, SQLite reads, and JSONL parsing do not freeze the Tauri window during startup or manual refresh.

- `RateWindow.usedPercent` comes from app-server and UI calculates remaining percent. It is shown only in official native mode.
- `OfficialUsage` comes from Codex app-server `account/usage/read`, including `dailyUsageBuckets`. It is the required source for official native mode token cards, account-level daily token buckets, and 7-day/30-day trend charts.
- `OfficialUsage.valuePeriod` is the fallback source for the membership-period value estimate card because app-server daily buckets expose aggregate token totals only.
- `LocalUsage.detailedUsage.valuePeriod` is the preferred source for membership-period value estimates when JSONL `token_count` events are available. It starts at the current billing-cycle date derived from `AppSettings.membershipStartedOn`, uses uncached input, cached input, and output token splits with model-specific official API prices, and remains zero when the current cycle has no parsed token events.
- `get_usage_snapshot` follows `AppSettings.accessMode`: official native mode reads official app-server data for quota/trend/account status and still parses local JSONL details for value estimates; API relay mode skips official app-server and parses local SQLite/JSONL as the primary dashboard data source.
- In official native mode, frontend token value and trend cards prefer `OfficialUsage`; if official usage is unavailable but `LocalUsage` exists, they fall back to local SQLite/JSONL data instead of showing zero. If official daily usage has not yet produced the current local day bucket and official today is zero, the UI uses JSONL detailed `LocalUsage.detailedUsage.today` for the today card and today's trend bar. It must not use `LocalUsage.dailyBuckets` for this supplement because those buckets aggregate full thread `tokens_used` for sessions updated that day.
- `TokenBreakdown.cachedInputTokens` is capped by UI formatting when displaying split bars.
- JSONL `token_count` events are cumulative per session; Rust stores deltas between events and resets on negative deltas.
- Task board groups active threads if updated in the last 2 hours, pending if touched today, done if archived today, scheduled if active automation TOML is found.
- Official 7-day trend windows are calendar-day buckets ending on the local current date. Missing dates are rendered as zero-token buckets so the chart remains stable.
- Official token value is an account-level estimate using aggregate token totals and the configured GPT-5 input token rate.
- `AppSettings.accessMode` records the selected Codex access display mode: official native login or API relay. Official native mode uses the default Codex app-server/account state and does not require or display an API endpoint.
- API relay fields are persisted `apiSiteName`, `apiEndpoint`, `apiModel`, `reasoningEffort`, and `speedMode`, plus transient `apiKey`. They are shown only for relay mode in the settings UI. The optional site name defaults to empty, is trimmed, has an optional user-entered `API：` prefix removed, and is capped at 40 characters; empty endpoint/path fields are normalized to `null`, an empty model name is normalized to `gpt-5` on save, relay endpoints are normalized to exactly one trailing `/v1`, and the dashboard uses local usage data because API users may not have official login data.
- `AppSettings.apiKey` is accepted by `save_app_settings` but is skipped during serialization. The encrypted vault stores it with the normalized endpoint; subsequent saves can leave the input empty and reuse it only for that same endpoint. A different endpoint requires a new Key.
- Model discovery is manual and read-only. It calls `GET /v1/models` only after the user clicks the settings button, never returns the Key to the UI, rejects cross-origin redirects, and filters the response to OpenAI GPT/ChatGPT/o-series/Codex model IDs while excluding moderation, safety, image generation, embedding, audio, and realtime families. The model field remains manually editable.
- Saving API relay mode updates the user Codex `config.toml` with the stable internal `model_provider = "qianzong_relay"` or, when unified history is enabled, `qianzong_unified`; the provider table uses the user-facing `name = "API：<apiSiteName>"`, or `API` when the optional name is empty, while keeping that internal ID stable for history migration. It also writes `base_url`, `wire_api = "responses"`, `preferred_auth_method = "apikey"`, and selected model/reasoning/speed fields. Codex `auth.json` is reduced to `auth_mode = "apikey"` plus `OPENAI_API_KEY` after the full official object has been captured to the encrypted vault.
- Saving official native mode edits the current Codex `config.toml` in place: it removes managed relay provider residue, restores the official provider and ChatGPT auth defaults, and preserves unrelated current config sections such as project paths/trust records and MCP servers. With a saved official login, unified-history mode may use `qianzong_unified`; without one, the config always uses `model_provider = "openai"` until Codex completes login. Relay endpoint/model/reasoning/speed preferences remain in app settings for the next switch; only the transient `apiKey` field is cleared. Codex `auth.json` is restored from the full saved official snapshot, including token and unknown fields, with `auth_mode = "chatgpt"` and `OPENAI_API_KEY = null`. If no official snapshot exists, switching still succeeds with an empty JSON object so Codex can start a new official login.
- `AppSettings.unifyCodexSessionHistory` is disabled by default. When enabled, both modes on the same machine use `qianzong_unified`; otherwise official uses `openai` and relay uses `qianzong_relay`. This does not synchronize history across devices. `unifyCodexMigrateExisting` explicitly opts into rewriting known `openai`/`qianzong_relay` provider IDs in session JSONL and `state_5.sqlite`, and migration is rejected while Codex is running.
- History migration backs up every changed JSONL file and SQLite database, records original provider IDs per session/thread, and skips unknown providers. Restore requires unified mode to be off and changes only records still marked `qianzong_unified`.
- The desktop app creates one `default-initial` managed backup of Codex `config.toml` and `auth.json` on first startup before later access-mode synchronization can rewrite those files. The settings drawer can create manual managed backups, select them from a dropdown, restore them, and delete non-default backups. Restore first creates timestamped backups of the current files, then copies backed-up files back; if a file did not exist in the selected snapshot, restoring that snapshot removes the current file to match the original state. `default-initial` is protected and cannot be deleted.
- `AppSettings.membershipStartedOn` is an optional `YYYY-MM-DD` original membership open date used only for current billing-cycle value calculation. Invalid or empty dates are normalized to `null`.
- `ReasoningEffort.extreme` maps to Codex `model_reasoning_effort = "xhigh"`. `ApiSpeedMode.fast` maps to `service_tier = "priority"`; stable/balanced remove the forced service tier.
- Skills board IPC returns `SkillBoard` / `SkillSummary` from `src-tauri/src/skills_board/`. The frontend passes only `skillId`; Rust rescans and resolves the path before any filesystem operation.
- Skills board metadata reads only bounded `SKILL.md` header/frontmatter content for `name` and `description`; full skill bodies are not sent to the frontend.
- Only user skills under the local Codex `~/.codex/skills` directory are disable/delete manageable on macOS. Disabled skills under `~/.codex/skills-disabled` are enable manageable. System skills, plugin cache skills, and `yonghu-preferences` are read-only. Delete is implemented as archive to `~/.codex/skills-trash`, not permanent removal.

## Error Policy

`get_usage_snapshot` prefers partial data over hard failure. Missing Codex CLI, missing SQLite, missing session logs, and app-server timeout are returned as diagnostics/messages.
