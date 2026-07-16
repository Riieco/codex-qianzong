use crate::{
    atomic_file,
    codex_config::{LEGACY_RELAY_PROVIDER_ID, SHARED_PROVIDER_ID},
    error::{AppError, AppResult},
    models::AppSettings,
    paths,
};
use chrono::{Local, Utc};
use rusqlite::{backup::Backup, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{Duration, SystemTime},
};

const MIGRATION_VERSION: u32 = 1;
const OFFICIAL_PROVIDER_ID: &str = "openai";
static HISTORY_OP_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMigrationOutcome {
    pub migrated_jsonl_files: usize,
    pub migrated_state_rows: usize,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRestoreOutcome {
    pub restored_jsonl_files: usize,
    pub restored_state_rows: usize,
    pub skipped_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MigrationManifest {
    version: u32,
    codex_dir: String,
    created_at: String,
    session_providers: BTreeMap<String, String>,
    thread_providers: BTreeMap<String, String>,
}

pub fn maybe_migrate(settings: &AppSettings) -> AppResult<HistoryMigrationOutcome> {
    if !settings.unify_codex_session_history {
        return Ok(skipped_migration("unify_disabled"));
    }
    if !settings.unify_codex_migrate_existing {
        return Ok(skipped_migration("migration_not_requested"));
    }
    let _guard = history_lock();
    let codex_dir = match paths::detect_codex_data_dir(settings) {
        Some(path) => path,
        None => return Ok(skipped_migration("codex_dir_not_found")),
    };
    if !live_config_uses_shared_provider(&codex_dir) {
        return Ok(skipped_migration("live_config_not_unified"));
    }
    let codex_dir_key = canonical_dir_string(&codex_dir);
    if marker_matches(&codex_dir_key)? {
        return Ok(skipped_migration("already_migrated"));
    }

    let generation = create_generation_dir("unified-v1")?;
    let mut manifest = MigrationManifest {
        version: MIGRATION_VERSION,
        codex_dir: codex_dir_key.clone(),
        created_at: Utc::now().to_rfc3339(),
        session_providers: BTreeMap::new(),
        thread_providers: BTreeMap::new(),
    };
    let migrated_jsonl_files = match migrate_jsonl_history(&codex_dir, &generation, &mut manifest) {
        Ok(count) => count,
        Err(err) => {
            write_json_atomic(&generation.join("manifest.json"), &manifest)?;
            return Err(err);
        }
    };
    write_json_atomic(&generation.join("manifest.json"), &manifest)?;
    let migrated_state_rows = match migrate_state_history(&codex_dir, &generation, &mut manifest) {
        Ok(count) => count,
        Err(err) => {
            write_json_atomic(&generation.join("manifest.json"), &manifest)?;
            return Err(err);
        }
    };
    write_json_atomic(&generation.join("manifest.json"), &manifest)?;
    write_json_atomic(&marker_path()?, &manifest)?;

    Ok(HistoryMigrationOutcome {
        migrated_jsonl_files,
        migrated_state_rows,
        skipped_reason: None,
    })
}

pub fn clear_marker() -> AppResult<()> {
    let path = marker_path()?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err.into()),
    }
}

pub fn has_backup(settings: &AppSettings) -> AppResult<bool> {
    let Some(codex_dir) = paths::detect_codex_data_dir(settings) else {
        return Ok(false);
    };
    let key = canonical_dir_string(&codex_dir);
    Ok(load_ledger(&key)?.is_some())
}

pub fn restore(settings: &AppSettings) -> AppResult<HistoryRestoreOutcome> {
    if settings.unify_codex_session_history {
        return Ok(skipped_restore("unify_enabled"));
    }
    let _guard = history_lock();
    let Some(codex_dir) = paths::detect_codex_data_dir(settings) else {
        return Ok(skipped_restore("codex_dir_not_found"));
    };
    let key = canonical_dir_string(&codex_dir);
    let Some(ledger) = load_ledger(&key)? else {
        return Ok(skipped_restore("no_backup_ledger"));
    };
    let generation = create_generation_dir("restore-v1")?;
    let restored_jsonl_files = restore_jsonl_history(&codex_dir, &generation, &ledger)?;
    let restored_state_rows = restore_state_history(&codex_dir, &generation, &ledger)?;
    clear_marker()?;

    if restored_jsonl_files == 0 && restored_state_rows == 0 {
        return Ok(skipped_restore("nothing_to_restore"));
    }
    Ok(HistoryRestoreOutcome {
        restored_jsonl_files,
        restored_state_rows,
        skipped_reason: None,
    })
}

fn history_lock() -> MutexGuard<'static, ()> {
    HISTORY_OP_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn skipped_migration(reason: &str) -> HistoryMigrationOutcome {
    HistoryMigrationOutcome {
        skipped_reason: Some(reason.to_string()),
        ..Default::default()
    }
}

fn skipped_restore(reason: &str) -> HistoryRestoreOutcome {
    HistoryRestoreOutcome {
        skipped_reason: Some(reason.to_string()),
        ..Default::default()
    }
}

fn live_config_uses_shared_provider(codex_dir: &Path) -> bool {
    let text = fs::read_to_string(codex_dir.join("config.toml")).unwrap_or_default();
    text.parse::<toml_edit::DocumentMut>()
        .ok()
        .and_then(|doc| doc.get("model_provider")?.as_str().map(str::to_string))
        .as_deref()
        == Some(SHARED_PROVIDER_ID)
}

fn marker_matches(codex_dir: &str) -> AppResult<bool> {
    let path = marker_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let manifest: MigrationManifest = serde_json::from_str(&fs::read_to_string(path)?)?;
    Ok(manifest.version == MIGRATION_VERSION && manifest.codex_dir == codex_dir)
}

fn marker_path() -> AppResult<PathBuf> {
    Ok(app_data_dir()?.join("codex-history-unified-marker.json"))
}

fn backup_parent() -> AppResult<PathBuf> {
    Ok(app_data_dir()?.join("codex-history-backups"))
}

fn app_data_dir() -> AppResult<PathBuf> {
    Ok(paths::app_log_dir()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".")))
}

fn create_generation_dir(kind: &str) -> AppResult<PathBuf> {
    let id = Local::now().format("%Y%m%d%H%M%S%3f");
    let path = backup_parent()?.join(kind).join(id.to_string());
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn migrate_jsonl_history(
    codex_dir: &Path,
    generation: &Path,
    manifest: &mut MigrationManifest,
) -> AppResult<usize> {
    let mut files = Vec::new();
    collect_jsonl_files(&codex_dir.join("sessions"), &mut files, 0, 8);
    collect_jsonl_files(&codex_dir.join("archived_sessions"), &mut files, 0, 5);
    let mut changed_files = 0;
    for path in files {
        if rewrite_jsonl_file(&path, codex_dir, generation, |value| {
            rewrite_session_meta_to_shared(value, &mut manifest.session_providers)
        })? {
            changed_files += 1;
        }
    }
    Ok(changed_files)
}

fn restore_jsonl_history(
    codex_dir: &Path,
    generation: &Path,
    ledger: &MigrationManifest,
) -> AppResult<usize> {
    let mut files = Vec::new();
    collect_jsonl_files(&codex_dir.join("sessions"), &mut files, 0, 8);
    collect_jsonl_files(&codex_dir.join("archived_sessions"), &mut files, 0, 5);
    let mut changed_files = 0;
    for path in files {
        if rewrite_jsonl_file(&path, codex_dir, generation, |value| {
            restore_session_meta(value, &ledger.session_providers)
        })? {
            changed_files += 1;
        }
    }
    Ok(changed_files)
}

fn rewrite_jsonl_file(
    path: &Path,
    codex_dir: &Path,
    generation: &Path,
    mut rewrite: impl FnMut(&mut Value) -> bool,
) -> AppResult<bool> {
    let metadata = fs::metadata(path)?;
    let modified = metadata.modified().ok();
    let length = metadata.len();
    let content = fs::read_to_string(path)?;
    let mut output = String::with_capacity(content.len());
    let mut changed = false;

    for segment in content.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        let mut value = serde_json::from_str::<Value>(line).ok();
        if value.as_mut().is_some_and(&mut rewrite) {
            output.push_str(&serde_json::to_string(value.as_ref().unwrap())?);
            changed = true;
        } else {
            output.push_str(line);
        }
        output.push_str(newline);
    }
    if !changed {
        return Ok(false);
    }

    ensure_unchanged(path, modified, length)?;
    backup_file(path, codex_dir, &generation.join("jsonl"))?;
    ensure_unchanged(path, modified, length)?;
    write_bytes_atomic(path, output.as_bytes())?;
    Ok(true)
}

fn rewrite_session_meta_to_shared(
    value: &mut Value,
    ledger: &mut BTreeMap<String, String>,
) -> bool {
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return false;
    }
    let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) else {
        return false;
    };
    let Some(id) = payload
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
    else {
        return false;
    };
    let Some(provider) = payload
        .get("model_provider")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return false;
    };
    if provider != OFFICIAL_PROVIDER_ID && provider != LEGACY_RELAY_PROVIDER_ID {
        return false;
    }
    ledger.entry(id).or_insert(provider);
    payload.insert(
        "model_provider".to_string(),
        Value::String(SHARED_PROVIDER_ID.to_string()),
    );
    true
}

fn restore_session_meta(value: &mut Value, ledger: &BTreeMap<String, String>) -> bool {
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return false;
    }
    let Some(payload) = value.get_mut("payload").and_then(Value::as_object_mut) else {
        return false;
    };
    if payload.get("model_provider").and_then(Value::as_str) != Some(SHARED_PROVIDER_ID) {
        return false;
    }
    let Some(original) = payload
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| ledger.get(id))
        .cloned()
    else {
        return false;
    };
    payload.insert("model_provider".to_string(), Value::String(original));
    true
}

fn migrate_state_history(
    codex_dir: &Path,
    generation: &Path,
    manifest: &mut MigrationManifest,
) -> AppResult<usize> {
    let mut changed = 0;
    for path in state_db_paths(codex_dir) {
        let mut conn = open_state_db(&path)?;
        if !threads_have_provider(&conn)? {
            continue;
        }
        let rows = read_source_threads(&conn)?;
        if rows.is_empty() {
            continue;
        }
        backup_database(&conn, &path, codex_dir, &generation.join("state"))?;
        let tx = conn.transaction()?;
        changed += tx.execute(
            "UPDATE threads SET model_provider = ?1 WHERE model_provider IN (?2, ?3)",
            (
                SHARED_PROVIDER_ID,
                OFFICIAL_PROVIDER_ID,
                LEGACY_RELAY_PROVIDER_ID,
            ),
        )?;
        tx.commit()?;
        for (id, provider) in rows {
            manifest.thread_providers.entry(id).or_insert(provider);
        }
    }
    Ok(changed)
}

fn restore_state_history(
    codex_dir: &Path,
    generation: &Path,
    ledger: &MigrationManifest,
) -> AppResult<usize> {
    let mut changed = 0;
    for path in state_db_paths(codex_dir) {
        let mut conn = open_state_db(&path)?;
        if !threads_have_provider(&conn)? {
            continue;
        }
        let matching: i64 = conn.query_row(
            "SELECT COUNT(*) FROM threads WHERE model_provider = ?1",
            [SHARED_PROVIDER_ID],
            |row| row.get(0),
        )?;
        if matching == 0 || ledger.thread_providers.is_empty() {
            continue;
        }
        backup_database(&conn, &path, codex_dir, &generation.join("state"))?;
        let tx = conn.transaction()?;
        for (id, provider) in &ledger.thread_providers {
            changed += tx.execute(
                "UPDATE threads SET model_provider = ?1 WHERE id = ?2 AND model_provider = ?3",
                (provider, id, SHARED_PROVIDER_ID),
            )?;
        }
        tx.commit()?;
    }
    Ok(changed)
}

fn open_state_db(path: &Path) -> AppResult<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    Ok(conn)
}

fn threads_have_provider(conn: &Connection) -> AppResult<bool> {
    let exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='threads'",
        [],
        |row| row.get(0),
    )?;
    if exists == 0 {
        return Ok(false);
    }
    let mut stmt = conn.prepare("PRAGMA table_info(threads)")?;
    let columns = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(columns.iter().any(|column| column == "model_provider"))
}

fn read_source_threads(conn: &Connection) -> AppResult<Vec<(String, String)>> {
    let mut stmt =
        conn.prepare("SELECT id, model_provider FROM threads WHERE model_provider IN (?1, ?2)")?;
    let rows = stmt
        .query_map((OFFICIAL_PROVIDER_ID, LEGACY_RELAY_PROVIDER_ID), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn backup_database(
    source: &Connection,
    source_path: &Path,
    codex_dir: &Path,
    backup_root: &Path,
) -> AppResult<()> {
    let target = backup_root.join(relative_path(source_path, codex_dir));
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut target_conn = Connection::open(target)?;
    let backup = Backup::new(source, &mut target_conn)?;
    backup.run_to_completion(5, Duration::from_millis(25), None)?;
    Ok(())
}

fn state_db_paths(codex_dir: &Path) -> Vec<PathBuf> {
    [
        codex_dir.join("state_5.sqlite"),
        codex_dir.join("sqlite").join("state_5.sqlite"),
    ]
    .into_iter()
    .filter(|path| path.exists())
    .collect()
}

fn load_ledger(codex_dir: &str) -> AppResult<Option<MigrationManifest>> {
    let root = backup_parent()?.join("unified-v1");
    if !root.exists() {
        return Ok(None);
    }
    let mut combined = MigrationManifest {
        version: MIGRATION_VERSION,
        codex_dir: codex_dir.to_string(),
        created_at: Utc::now().to_rfc3339(),
        session_providers: BTreeMap::new(),
        thread_providers: BTreeMap::new(),
    };
    for entry in fs::read_dir(root)?.flatten() {
        let path = entry.path().join("manifest.json");
        if !path.exists() {
            continue;
        }
        let manifest: MigrationManifest = match serde_json::from_str(&fs::read_to_string(path)?) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if manifest.version != MIGRATION_VERSION || manifest.codex_dir != codex_dir {
            continue;
        }
        for (id, provider) in manifest.session_providers {
            combined.session_providers.entry(id).or_insert(provider);
        }
        for (id, provider) in manifest.thread_providers {
            combined.thread_providers.entry(id).or_insert(provider);
        }
    }
    if combined.session_providers.is_empty() && combined.thread_providers.is_empty() {
        Ok(None)
    } else {
        Ok(Some(combined))
    }
}

fn collect_jsonl_files(dir: &Path, files: &mut Vec<PathBuf>, depth: u8, max_depth: u8) {
    if depth > max_depth || !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|value| value.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

fn backup_file(path: &Path, root: &Path, backup_root: &Path) -> AppResult<()> {
    let target = backup_root.join(relative_path(path, root));
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(path, target)?;
    Ok(())
}

fn relative_path(path: &Path, root: &Path) -> PathBuf {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .unwrap_or_else(|_| path.file_name().map(PathBuf::from).unwrap_or_default())
}

fn ensure_unchanged(path: &Path, modified: Option<SystemTime>, length: u64) -> AppResult<()> {
    let current = fs::metadata(path)?;
    if current.modified().ok() != modified || current.len() != length {
        return Err(AppError::Config(format!(
            "迁移期间 Codex 会话文件发生变化: {}",
            path.display()
        )));
    }
    Ok(())
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> AppResult<()> {
    write_bytes_atomic(
        path,
        format!("{}\n", serde_json::to_string_pretty(value)?).as_bytes(),
    )
}

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> AppResult<()> {
    atomic_file::write(path, bytes).map_err(Into::into)
}

fn canonical_dir_string(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrites_only_known_provider_session_meta() {
        let mut ledger = BTreeMap::new();
        let mut official = serde_json::json!({
            "type": "session_meta",
            "payload": { "id": "s1", "model_provider": "openai" }
        });
        let mut unknown = serde_json::json!({
            "type": "session_meta",
            "payload": { "id": "s2", "model_provider": "private" }
        });

        assert!(rewrite_session_meta_to_shared(&mut official, &mut ledger));
        assert!(!rewrite_session_meta_to_shared(&mut unknown, &mut ledger));
        assert_eq!(
            official.pointer("/payload/model_provider"),
            Some(&Value::String(SHARED_PROVIDER_ID.into()))
        );
        assert_eq!(ledger.get("s1").map(String::as_str), Some("openai"));
    }

    #[test]
    fn sqlite_history_migration_and_restore_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let codex_dir = temp.path().join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let db_path = codex_dir.join("state_5.sqlite");
        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch(
            "CREATE TABLE threads (id TEXT PRIMARY KEY, model_provider TEXT NOT NULL);
             INSERT INTO threads VALUES ('official', 'openai');
             INSERT INTO threads VALUES ('relay', 'qianzong_relay');
             INSERT INTO threads VALUES ('other', 'private');",
        )
        .unwrap();
        drop(conn);
        let mut manifest = MigrationManifest {
            version: MIGRATION_VERSION,
            codex_dir: canonical_dir_string(&codex_dir),
            created_at: Utc::now().to_rfc3339(),
            session_providers: BTreeMap::new(),
            thread_providers: BTreeMap::new(),
        };
        let migration_backup = temp.path().join("migration");
        assert_eq!(
            migrate_state_history(&codex_dir, &migration_backup, &mut manifest).unwrap(),
            2
        );
        let restore_backup = temp.path().join("restore");
        assert_eq!(
            restore_state_history(&codex_dir, &restore_backup, &manifest).unwrap(),
            2
        );
        let conn = Connection::open(db_path).unwrap();
        let official: String = conn
            .query_row(
                "SELECT model_provider FROM threads WHERE id='official'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let relay: String = conn
            .query_row(
                "SELECT model_provider FROM threads WHERE id='relay'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(official, "openai");
        assert_eq!(relay, "qianzong_relay");
    }

    #[test]
    fn jsonl_history_migration_and_restore_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let codex_dir = temp.path().join(".codex");
        let sessions = codex_dir.join("sessions/2026/07/16");
        fs::create_dir_all(&sessions).unwrap();
        let session_path = sessions.join("rollout.jsonl");
        fs::write(
            &session_path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"official\",\"model_provider\":\"openai\",\"cwd\":\"C:/work\"}}\n",
                "{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\"}}\n",
                "{\"type\":\"session_meta\",\"payload\":{\"model_provider\":\"openai\"}}\n",
            ),
        )
        .unwrap();
        let original = fs::read_to_string(&session_path).unwrap();
        let mut manifest = MigrationManifest {
            version: MIGRATION_VERSION,
            codex_dir: canonical_dir_string(&codex_dir),
            created_at: Utc::now().to_rfc3339(),
            session_providers: BTreeMap::new(),
            thread_providers: BTreeMap::new(),
        };

        assert_eq!(
            migrate_jsonl_history(&codex_dir, &temp.path().join("migration"), &mut manifest)
                .unwrap(),
            1
        );
        let migrated = fs::read_to_string(&session_path).unwrap();
        assert!(migrated.contains("qianzong_unified"));
        assert_eq!(migrated.matches("qianzong_unified").count(), 1);
        assert_eq!(
            restore_jsonl_history(&codex_dir, &temp.path().join("restore"), &manifest).unwrap(),
            1
        );
        let restored = fs::read_to_string(session_path).unwrap();
        let parse_lines = |text: &str| {
            text.lines()
                .map(|line| serde_json::from_str::<Value>(line).unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(parse_lines(&restored), parse_lines(&original));
    }
}
