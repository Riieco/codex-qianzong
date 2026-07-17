use crate::{
    atomic_file,
    auth_vault::{AuthCredentialStatus, AuthVaultStore, RelayCredential, SystemAuthVault},
    error::{AppError, AppResult},
    models::{
        api_provider_display_name, ApiSpeedMode, AppSettings, CodexAccessMode, CodexConfigBackup,
        ReasoningEffort,
    },
    paths,
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
use toml_edit::{value, DocumentMut, Item, Table};

pub const SHARED_PROVIDER_ID: &str = "qianzong_unified";
pub const LEGACY_RELAY_PROVIDER_ID: &str = "qianzong_relay";
const OFFICIAL_MODEL: &str = "gpt-5.5";
const DEFAULT_BACKUP_ID: &str = "default-initial";
const DEFAULT_BACKUP_LABEL: &str = "首次启动默认配置";
const REPAIRABLE_DUPLICATE_TABLES: &[&str] = &["desktop.appearanceDarkChromeTheme"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexConfigBackupManifest {
    id: String,
    label: String,
    created_at: String,
    is_default: bool,
    has_config: bool,
    has_auth: bool,
}

pub fn sync_codex_config(settings: &AppSettings) -> AppResult<()> {
    let codex_dir = paths::resolve_codex_data_dir(settings)?;
    let config_path = codex_config_path(&codex_dir);
    let auth_path = codex_auth_path(&codex_dir);
    let restore_path = restore_snapshot_path()?;
    ensure_default_backup_for_paths(&codex_dir, &config_path, &auth_path)?;
    let official_auth_fallback = if settings.access_mode == CodexAccessMode::Official {
        find_official_auth_fallback(&codex_dir, &auth_path)?
    } else {
        None
    };
    sync_codex_config_for_paths(
        settings,
        &config_path,
        &auth_path,
        &restore_path,
        &SystemAuthVault,
        official_auth_fallback.as_ref(),
    )
}

pub fn auth_credential_status() -> AppResult<AuthCredentialStatus> {
    SystemAuthVault.status()
}

pub fn clear_stored_relay_api_key() -> AppResult<AuthCredentialStatus> {
    SystemAuthVault.clear_relay()
}

pub fn capture_current_official_auth() -> AppResult<()> {
    let codex_dir = active_codex_data_dir()?;
    capture_current_auth_to_vault(&codex_auth_path(&codex_dir), &SystemAuthVault)
}

fn active_codex_data_dir() -> AppResult<PathBuf> {
    paths::resolve_codex_data_dir(&crate::settings::read_settings().unwrap_or_default())
}

fn codex_config_path(codex_dir: &Path) -> PathBuf {
    codex_dir.join("config.toml")
}

fn codex_auth_path(codex_dir: &Path) -> PathBuf {
    codex_dir.join("auth.json")
}

fn restore_snapshot_path() -> AppResult<PathBuf> {
    let app_dir = paths::app_log_dir()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(app_dir.join("codex-config-restore.toml"))
}

fn sync_codex_config_for_paths(
    settings: &AppSettings,
    config_path: &Path,
    auth_path: &Path,
    restore_path: &Path,
    vault: &dyn AuthVaultStore,
    official_auth_fallback: Option<&Map<String, Value>>,
) -> AppResult<()> {
    let original = read_optional_text(config_path)?;
    let original_auth = read_optional_text(auth_path)?;
    let next_auth = prepare_codex_auth(
        settings,
        &original,
        &original_auth,
        vault,
        official_auth_fallback,
    )?;

    let next_text = match settings.access_mode {
        CodexAccessMode::Relay => {
            ensure_restore_snapshot(config_path, restore_path, &original)?;
            let mut doc = parse_config(&original)?;
            apply_relay_config(&mut doc, settings)?;
            doc.to_string()
        }
        CodexAccessMode::Official => {
            let mut doc = parse_config(&original)?;
            apply_official_config(&mut doc, settings, !next_auth.is_empty());
            doc.to_string()
        }
    };

    if next_text != original {
        backup_existing_file(config_path, "config.toml")?;
        write_config(config_path, &next_text)?;
    }

    write_codex_auth_if_changed(auth_path, &original_auth, &next_auth)?;

    Ok(())
}

pub fn ensure_default_codex_config_backup() -> AppResult<()> {
    let codex_dir = active_codex_data_dir()?;
    let config_path = codex_config_path(&codex_dir);
    let auth_path = codex_auth_path(&codex_dir);
    ensure_default_backup_for_paths(&codex_dir, &config_path, &auth_path)
}

pub fn list_codex_config_backups() -> AppResult<Vec<CodexConfigBackup>> {
    let codex_dir = active_codex_data_dir()?;
    ensure_default_codex_config_backup()?;
    let mut backups = read_backup_manifests(&codex_dir)?;
    backups.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    Ok(backups)
}

pub fn create_codex_config_backup(label: Option<String>) -> AppResult<Vec<CodexConfigBackup>> {
    let codex_dir = active_codex_data_dir()?;
    let config_path = codex_config_path(&codex_dir);
    let auth_path = codex_auth_path(&codex_dir);
    let timestamp = Local::now().format("%Y%m%d%H%M%S%3f").to_string();
    let label = label
        .and_then(|value| {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        })
        .unwrap_or_else(|| format!("手动备份 {}", Local::now().format("%Y-%m-%d %H:%M:%S")));
    create_backup_snapshot(
        &format!("manual-{timestamp}"),
        &label,
        false,
        &codex_dir,
        &config_path,
        &auth_path,
    )?;
    list_codex_config_backups()
}

pub fn restore_codex_config_backup(id: &str) -> AppResult<Vec<CodexConfigBackup>> {
    let codex_dir = active_codex_data_dir()?;
    let backup_dir = backup_dir_for_id(&codex_dir, id)?;
    let manifest = read_backup_manifest(&backup_dir)?;
    let config_path = codex_config_path(&codex_dir);
    let auth_path = codex_auth_path(&codex_dir);

    restore_snapshot_entry(
        &backup_dir.join("config.toml"),
        &config_path,
        "config.toml",
        manifest.has_config,
    )?;
    restore_snapshot_entry(
        &backup_dir.join("auth.json"),
        &auth_path,
        "auth.json",
        manifest.has_auth,
    )?;

    capture_current_auth_to_vault(&auth_path, &SystemAuthVault)?;

    list_codex_config_backups()
}

pub fn delete_codex_config_backup(id: &str) -> AppResult<Vec<CodexConfigBackup>> {
    let codex_dir = active_codex_data_dir()?;
    let backup_dir = backup_dir_for_id(&codex_dir, id)?;
    let manifest = read_backup_manifest(&backup_dir)?;
    if manifest.is_default || manifest.id == DEFAULT_BACKUP_ID {
        return Err(AppError::Config("默认配置备份不能删除".into()));
    }
    fs::remove_dir_all(backup_dir)?;
    list_codex_config_backups()
}

fn ensure_default_backup_for_paths(
    codex_dir: &Path,
    config_path: &Path,
    auth_path: &Path,
) -> AppResult<()> {
    let backup_dir = backup_dir_for_id(codex_dir, DEFAULT_BACKUP_ID)?;
    if backup_dir.join("manifest.json").exists() {
        return Ok(());
    }
    create_backup_snapshot(
        DEFAULT_BACKUP_ID,
        DEFAULT_BACKUP_LABEL,
        true,
        codex_dir,
        config_path,
        auth_path,
    )
}

fn backup_root_dir(codex_dir: &Path) -> AppResult<PathBuf> {
    let app_dir = paths::app_log_dir()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let root = app_dir.join("codex-config-backups");
    let default_dir = dirs::home_dir().map(|home| home.join(".codex"));
    if default_dir
        .as_deref()
        .is_some_and(|path| paths::canonical_path_key(path) == paths::canonical_path_key(codex_dir))
    {
        Ok(root)
    } else {
        Ok(root.join(format!("profile-{:016x}", stable_path_hash(codex_dir))))
    }
}

fn stable_path_hash(path: &Path) -> u64 {
    paths::canonical_path_key(path)
        .bytes()
        .fold(0xcbf29ce484222325, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        })
}

fn backup_dir_for_id(codex_dir: &Path, id: &str) -> AppResult<PathBuf> {
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        return Err(AppError::Config("备份 ID 不合法".into()));
    }
    Ok(backup_root_dir(codex_dir)?.join(id))
}

fn create_backup_snapshot(
    id: &str,
    label: &str,
    is_default: bool,
    codex_dir: &Path,
    config_path: &Path,
    auth_path: &Path,
) -> AppResult<()> {
    let backup_dir = backup_dir_for_id(codex_dir, id)?;
    fs::create_dir_all(&backup_dir)?;

    let has_config = copy_if_exists(config_path, &backup_dir.join("config.toml"))?;
    let has_auth = copy_if_exists(auth_path, &backup_dir.join("auth.json"))?;
    let manifest = CodexConfigBackupManifest {
        id: id.to_string(),
        label: label.to_string(),
        created_at: Local::now().to_rfc3339(),
        is_default,
        has_config,
        has_auth,
    };
    let manifest_text = serde_json::to_string_pretty(&manifest)?;
    fs::write(
        backup_dir.join("manifest.json"),
        format!("{manifest_text}\n"),
    )?;
    Ok(())
}

fn copy_if_exists(source: &Path, target: &Path) -> AppResult<bool> {
    if !source.exists() {
        return Ok(false);
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, target)?;
    Ok(true)
}

fn restore_backup_file(source: &Path, target: &Path) -> AppResult<()> {
    if !source.exists() {
        return Err(AppError::Config("备份文件缺失，无法恢复".into()));
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(source, target)?;
    Ok(())
}

fn restore_snapshot_entry(
    source: &Path,
    target: &Path,
    base_name: &str,
    existed_in_backup: bool,
) -> AppResult<()> {
    backup_existing_file(target, base_name)?;
    if existed_in_backup {
        restore_backup_file(source, target)
    } else {
        match fs::remove_file(target) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

fn read_backup_manifests(codex_dir: &Path) -> AppResult<Vec<CodexConfigBackup>> {
    let root = backup_root_dir(codex_dir)?;
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut backups = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        if let Ok(manifest) = read_backup_manifest(&entry.path()) {
            backups.push(CodexConfigBackup {
                id: manifest.id,
                label: manifest.label,
                created_at: manifest.created_at,
                is_default: manifest.is_default,
                has_config: manifest.has_config,
                has_auth: manifest.has_auth,
            });
        }
    }
    Ok(backups)
}

fn read_backup_manifest(backup_dir: &Path) -> AppResult<CodexConfigBackupManifest> {
    let text = fs::read_to_string(backup_dir.join("manifest.json"))?;
    serde_json::from_str(&text).map_err(|err| AppError::Config(format!("备份清单解析失败: {err}")))
}

fn read_optional_text(path: &Path) -> AppResult<String> {
    if path.exists() {
        Ok(fs::read_to_string(path)?)
    } else {
        Ok(String::new())
    }
}

fn parse_config(text: &str) -> AppResult<DocumentMut> {
    if text.trim().is_empty() {
        return Ok(DocumentMut::new());
    }

    match text.parse::<DocumentMut>() {
        Ok(doc) => Ok(doc),
        Err(original_error) => {
            let repairable = is_repairable_duplicate_error(&original_error.to_string());
            repairable
                .then(|| repair_known_duplicate_tables(text))
                .flatten()
                .and_then(|repaired| repaired.parse::<DocumentMut>().ok())
                .ok_or_else(|| AppError::Config(format!("Codex 配置解析失败: {original_error}")))
        }
    }
}

fn is_repairable_duplicate_error(message: &str) -> bool {
    message.contains("duplicate key")
        && REPAIRABLE_DUPLICATE_TABLES
            .iter()
            .any(|table| message.contains(table))
}

fn repair_known_duplicate_tables(text: &str) -> Option<String> {
    let lines = text.split_inclusive('\n').collect::<Vec<_>>();
    let mut first_expanded_tables = HashMap::new();
    for (index, line) in lines.iter().enumerate() {
        if let Some(table) = repairable_table_header(line) {
            first_expanded_tables.entry(table).or_insert(index);
        }
    }
    if first_expanded_tables.is_empty() {
        return None;
    }

    let mut seen = HashSet::new();
    let mut repaired = false;
    let mut output = String::with_capacity(text.len());
    let mut current_table = String::new();

    for (index, line) in lines.into_iter().enumerate() {
        if let Some(table_path) = table_header_path(line) {
            current_table = table_path;
            if let Some(table) = REPAIRABLE_DUPLICATE_TABLES
                .iter()
                .copied()
                .find(|known| *known == current_table)
            {
                if !seen.insert(table) {
                    repaired = true;
                    continue;
                }
            }
        } else if let Some(table) = repairable_inline_assignment(line, &current_table) {
            if first_expanded_tables
                .get(table)
                .is_some_and(|header_index| index < *header_index)
            {
                repaired = true;
                continue;
            }
        }
        output.push_str(line);
    }

    repaired.then_some(output)
}

fn repairable_table_header(line: &str) -> Option<&'static str> {
    let normalized = table_header_path(line)?;
    REPAIRABLE_DUPLICATE_TABLES
        .iter()
        .copied()
        .find(|known| *known == normalized)
}

fn table_header_path(line: &str) -> Option<String> {
    let code = line.split('#').next()?.trim();
    if code.starts_with("[[") || !code.starts_with('[') || !code.ends_with(']') {
        return None;
    }
    Some(normalize_dotted_key(&code[1..code.len() - 1]))
}

fn repairable_inline_assignment(line: &str, current_table: &str) -> Option<&'static str> {
    let (key, _) = line.trim().split_once('=')?;
    let key = normalize_dotted_key(key);
    let path = if current_table.is_empty() {
        key
    } else {
        format!("{current_table}.{key}")
    };
    REPAIRABLE_DUPLICATE_TABLES
        .iter()
        .copied()
        .find(|known| *known == path)
}

fn normalize_dotted_key(key: &str) -> String {
    key.split('.').map(str::trim).collect::<Vec<_>>().join(".")
}

fn ensure_restore_snapshot(
    config_path: &Path,
    restore_path: &Path,
    current_text: &str,
) -> AppResult<()> {
    if restore_path.exists() || !config_path.exists() || is_qianzong_managed(current_text) {
        return Ok(());
    }
    if let Some(parent) = restore_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(restore_path, current_text)?;
    Ok(())
}

fn write_codex_auth_if_changed(
    auth_path: &Path,
    original: &str,
    auth: &Map<String, Value>,
) -> AppResult<()> {
    let next_text = serde_json::to_string_pretty(&Value::Object(auth.clone()))?;
    if next_text != original.trim() {
        backup_existing_auth_file(auth_path, original)?;
        write_config(auth_path, &format!("{next_text}\n"))?;
    }

    Ok(())
}

fn backup_existing_auth_file(path: &Path, original: &str) -> AppResult<()> {
    let auth = parse_auth_json(original)?;
    if auth.get("auth_mode").and_then(Value::as_str) == Some("apikey") {
        return Ok(());
    }
    backup_existing_file(path, "auth.json")
}

fn prepare_codex_auth(
    settings: &AppSettings,
    current_config: &str,
    current_auth: &str,
    vault: &dyn AuthVaultStore,
    official_auth_fallback: Option<&Map<String, Value>>,
) -> AppResult<Map<String, Value>> {
    let current = parse_auth_json(current_auth)?;
    let mut data = vault.load()?;
    let mut vault_changed = data.capture_official_auth(&current);
    if data.official_auth_map().is_none() {
        if let Some(fallback) = official_auth_fallback {
            vault_changed |= data.capture_official_auth(fallback);
        }
    }
    if let (Some(endpoint), Some(api_key)) = (
        managed_relay_endpoint(current_config),
        current
            .get("OPENAI_API_KEY")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) {
        let current_relay = RelayCredential {
            endpoint,
            api_key: api_key.to_string(),
        };
        if data.relay.as_ref() != Some(&current_relay) {
            data.relay = Some(current_relay);
            vault_changed = true;
        }
    }

    let next = match settings.access_mode {
        CodexAccessMode::Relay => {
            let endpoint = settings
                .api_endpoint
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| AppError::Config("API 中转模式需要填写 API 地址".into()))?;
            let provided_key = settings
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());

            let key = if let Some(key) = provided_key {
                data.relay = Some(RelayCredential {
                    endpoint: endpoint.to_string(),
                    api_key: key.to_string(),
                });
                vault_changed = true;
                key.to_string()
            } else if let Some(relay) = data.relay.as_ref() {
                if relay.endpoint != endpoint {
                    return Err(AppError::Config(
                        "API 地址已变化，请重新输入该地址对应的 API Key".into(),
                    ));
                }
                relay.api_key.clone()
            } else if managed_relay_endpoint(current_config).as_deref() == Some(endpoint) {
                let key = current
                    .get("OPENAI_API_KEY")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| AppError::Config("API 中转模式需要填写 API Key".into()))?;
                data.relay = Some(RelayCredential {
                    endpoint: endpoint.to_string(),
                    api_key: key.to_string(),
                });
                vault_changed = true;
                key.to_string()
            } else {
                return Err(AppError::Config("API 中转模式需要填写 API Key".into()));
            };

            Map::from_iter([
                ("auth_mode".to_string(), Value::String("apikey".to_string())),
                ("OPENAI_API_KEY".to_string(), Value::String(key)),
            ])
        }
        CodexAccessMode::Official => {
            let official = data.official_auth_map().or_else(|| {
                current
                    .get("auth_mode")
                    .and_then(Value::as_str)
                    .filter(|mode| *mode == "chatgpt")
                    .map(|_| current.clone())
            });
            official.map_or_else(Map::new, |mut auth| {
                auth.insert(
                    "auth_mode".to_string(),
                    Value::String("chatgpt".to_string()),
                );
                auth.insert("OPENAI_API_KEY".to_string(), Value::Null);
                auth
            })
        }
    };

    if vault_changed {
        vault.save(&data)?;
    }
    Ok(next)
}

fn find_official_auth_fallback(
    codex_dir: &Path,
    auth_path: &Path,
) -> AppResult<Option<Map<String, Value>>> {
    let mut candidates = vec![backup_dir_for_id(codex_dir, DEFAULT_BACKUP_ID)?.join("auth.json")];
    if let Ok(entries) = fs::read_dir(codex_dir) {
        let mut automatic = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("auth.json.qianzong-backup-"))
            })
            .collect::<Vec<_>>();
        automatic.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        candidates.extend(automatic);
    }

    for path in candidates {
        if path == auth_path || !path.is_file() {
            continue;
        }
        if let Some(auth) = read_official_auth_candidate(&path) {
            return Ok(Some(auth));
        }
    }
    Ok(None)
}

fn read_official_auth_candidate(path: &Path) -> Option<Map<String, Value>> {
    let text = fs::read_to_string(path).ok()?;
    let auth = parse_auth_json(&text).ok()?;
    let mut data = crate::auth_vault::AuthVaultData::default();
    data.capture_official_auth(&auth)
        .then(|| data.official_auth_map())
        .flatten()
}

fn capture_current_auth_to_vault(auth_path: &Path, vault: &dyn AuthVaultStore) -> AppResult<()> {
    let current = parse_auth_json(&read_optional_text(auth_path)?)?;
    let mut data = vault.load()?;
    if data.capture_official_auth(&current) {
        vault.save(&data)?;
    }
    Ok(())
}

fn parse_auth_json(text: &str) -> AppResult<Map<String, Value>> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    let value: Value = serde_json::from_str(text)
        .map_err(|err| AppError::Config(format!("Codex 认证文件解析失败: {err}")))?;
    value
        .as_object()
        .cloned()
        .ok_or_else(|| AppError::Config("Codex 认证文件必须是 JSON 对象".into()))
}

fn backup_existing_file(path: &Path, base_name: &str) -> AppResult<()> {
    if !path.exists() {
        return Ok(());
    }
    let timestamp = Local::now().format("%Y%m%d%H%M%S%3f");
    let backup_name = format!("{base_name}.qianzong-backup-{timestamp}");
    let backup_path = path.with_file_name(backup_name);
    fs::copy(path, backup_path)?;
    Ok(())
}

fn write_config(config_path: &Path, text: &str) -> AppResult<()> {
    atomic_file::write(config_path, text.as_bytes()).map_err(Into::into)
}

fn apply_relay_config(doc: &mut DocumentMut, settings: &AppSettings) -> AppResult<()> {
    let endpoint = settings
        .api_endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| AppError::Config("API 中转模式需要填写 API 地址".into()))?;
    let model = settings.api_model.trim();

    doc["model"] = value(if model.is_empty() { "gpt-5" } else { model });
    let provider_id = if settings.unify_codex_session_history {
        SHARED_PROVIDER_ID
    } else {
        LEGACY_RELAY_PROVIDER_ID
    };
    doc["model_provider"] = value(provider_id);
    doc["preferred_auth_method"] = value("apikey");
    doc["model_reasoning_effort"] = value(reasoning_effort_value(&settings.reasoning_effort));

    match settings.speed_mode {
        ApiSpeedMode::Fast => {
            doc["service_tier"] = value("priority");
        }
        ApiSpeedMode::Stable | ApiSpeedMode::Balanced => {
            doc.as_table_mut().remove("service_tier");
        }
    }

    let relay = ensure_managed_provider_table(doc, provider_id)?;
    relay.clear();
    if let Some(display_name) = api_provider_display_name(&settings.api_site_name) {
        relay.insert("name", value(display_name));
    }
    relay.insert("base_url", value(endpoint));
    relay.insert("wire_api", value("responses"));
    Ok(())
}

fn apply_official_config(doc: &mut DocumentMut, settings: &AppSettings, has_official_auth: bool) {
    let root = doc.as_table_mut();
    root.remove("openai_base_url");
    root.remove("service_tier");
    root.insert("model", value(OFFICIAL_MODEL));
    root.insert("model_reasoning_effort", value("medium"));
    root.insert("preferred_auth_method", value("chatgpt"));

    if settings.unify_codex_session_history && has_official_auth {
        root.insert("model_provider", value(SHARED_PROVIDER_ID));
        if !matches!(root.get("model_providers"), Some(Item::Table(_))) {
            root.insert("model_providers", Item::Table(Table::new()));
        }
        let providers = root
            .get_mut("model_providers")
            .and_then(Item::as_table_mut)
            .expect("model_providers was initialized as a table");
        providers.remove(LEGACY_RELAY_PROVIDER_ID);
        let mut official = Table::new();
        official.insert("name", value("OpenAI"));
        official.insert("requires_openai_auth", value(true));
        official.insert("supports_websockets", value(true));
        official.insert("wire_api", value("responses"));
        providers.insert(SHARED_PROVIDER_ID, Item::Table(official));
    } else {
        root.insert("model_provider", value("openai"));
        let providers_empty = root
            .get_mut("model_providers")
            .and_then(Item::as_table_mut)
            .map(|providers| {
                providers.remove(LEGACY_RELAY_PROVIDER_ID);
                providers.remove(SHARED_PROVIDER_ID);
                providers.is_empty()
            })
            .unwrap_or(false);
        if providers_empty {
            root.remove("model_providers");
        }
    }
}

fn ensure_managed_provider_table<'a>(
    doc: &'a mut DocumentMut,
    provider_id: &str,
) -> AppResult<&'a mut Table> {
    let root = doc.as_table_mut();
    if !matches!(root.get("model_providers"), Some(Item::Table(_))) {
        root.insert("model_providers", Item::Table(Table::new()));
    }
    let providers = root
        .get_mut("model_providers")
        .and_then(Item::as_table_mut)
        .ok_or_else(|| AppError::Config("无法写入 Codex provider 配置".into()))?;
    let other_id = if provider_id == SHARED_PROVIDER_ID {
        LEGACY_RELAY_PROVIDER_ID
    } else {
        SHARED_PROVIDER_ID
    };
    providers.remove(other_id);
    if !matches!(providers.get(provider_id), Some(Item::Table(_))) {
        providers.insert(provider_id, Item::Table(Table::new()));
    }
    providers
        .get_mut(provider_id)
        .and_then(Item::as_table_mut)
        .ok_or_else(|| AppError::Config("无法写入 API 中转 provider 配置".into()))
}

fn managed_relay_endpoint(text: &str) -> Option<String> {
    let doc = text.parse::<DocumentMut>().ok()?;
    let provider_id = doc.get("model_provider")?.as_str()?;
    if provider_id != SHARED_PROVIDER_ID && provider_id != LEGACY_RELAY_PROVIDER_ID {
        return None;
    }
    doc.get("model_providers")?
        .as_table()?
        .get(provider_id)?
        .as_table()?
        .get("base_url")?
        .as_str()
        .map(str::to_string)
}

fn reasoning_effort_value(effort: &ReasoningEffort) -> &'static str {
    match effort {
        ReasoningEffort::Minimal | ReasoningEffort::Low => "low",
        ReasoningEffort::Medium => "medium",
        ReasoningEffort::High => "high",
        ReasoningEffort::Extreme => "xhigh",
    }
}

fn is_qianzong_managed(text: &str) -> bool {
    if let Ok(doc) = text.parse::<DocumentMut>() {
        let root = doc.as_table();
        if root
            .get("model_provider")
            .and_then(Item::as_value)
            .and_then(|item| item.as_str())
            .is_some_and(|id| id == SHARED_PROVIDER_ID || id == LEGACY_RELAY_PROVIDER_ID)
        {
            return true;
        }
        if root
            .get("model_providers")
            .and_then(Item::as_table)
            .is_some_and(|providers| {
                providers.contains_key(SHARED_PROVIDER_ID)
                    || providers.contains_key(LEGACY_RELAY_PROVIDER_ID)
            })
        {
            return true;
        }
    }
    text.contains("model_provider = \"qianzong_unified\"")
        || text.contains("[model_providers.qianzong_unified]")
        || text.contains("model_provider = \"qianzong_relay\"")
        || text.contains("[model_providers.qianzong_relay]")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_vault::MemoryAuthVault;

    fn sync_test(
        settings: &AppSettings,
        config_path: &Path,
        auth_path: &Path,
        restore_path: &Path,
    ) -> AppResult<()> {
        sync_codex_config_for_paths(
            settings,
            config_path,
            auth_path,
            restore_path,
            &MemoryAuthVault::default(),
            None,
        )
    }

    #[test]
    fn sync_repairs_duplicate_desktop_theme_table_and_keeps_its_content() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let auth_path = temp.path().join("auth.json");
        let restore_path = temp.path().join("restore.toml");
        fs::write(
            &config_path,
            r#"model = "gpt-5.5"

[desktop.appearanceDarkChromeTheme]
enabled = true

[ desktop.appearanceDarkChromeTheme ] # duplicate written by Codex desktop
contrast = "high"
"#,
        )
        .unwrap();

        sync_test(
            &AppSettings::default(),
            &config_path,
            &auth_path,
            &restore_path,
        )
        .unwrap();

        let text = fs::read_to_string(&config_path).unwrap();
        assert_eq!(
            text.matches("[desktop.appearanceDarkChromeTheme]").count(),
            1
        );
        assert!(text.contains("enabled = true"));
        assert!(text.contains(r#"contrast = "high""#));
        assert!(temp.path().read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("config.toml.qianzong-backup-")));
    }

    #[test]
    fn sync_prefers_expanded_desktop_theme_over_older_inline_theme() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let auth_path = temp.path().join("auth.json");
        let restore_path = temp.path().join("restore.toml");
        fs::write(
            &config_path,
            r##"model = "gpt-5.5"

[desktop]
appearanceDarkChromeTheme = { accent = "#63E6FF", contrast = 72 }

[desktop.appearanceDarkChromeTheme ]
accent = "#ff6363"
contrast = 60

[desktop.appearanceDarkChromeTheme.fonts]
code = '"Jetbrains Mono"'
ui = "Inter"
"##,
        )
        .unwrap();

        sync_test(
            &AppSettings::default(),
            &config_path,
            &auth_path,
            &restore_path,
        )
        .unwrap();

        let text = fs::read_to_string(&config_path).unwrap();
        assert!(!text.contains("#63E6FF"));
        assert!(text.contains(r##"accent = "#ff6363""##));
        assert!(text.contains("[desktop.appearanceDarkChromeTheme.fonts]"));
        assert!(text.contains(r#"ui = "Inter""#));
    }

    #[test]
    fn parse_config_keeps_unknown_duplicate_table_errors_strict() {
        let error = parse_config(
            r#"[unknown.table]
first = true

[unknown.table]
second = true
"#,
        )
        .unwrap_err();

        assert!(error.to_string().contains("duplicate key"));
    }

    #[test]
    fn relay_sync_writes_custom_provider_and_restore_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let auth_path = temp.path().join("auth.json");
        let restore_path = temp.path().join("codex-config-restore.toml");
        fs::write(
            &config_path,
            r#"model = "gpt-5.5"
preferred_auth_method = "chatgpt"
"#,
        )
        .unwrap();

        let mut settings = AppSettings::default();
        settings.access_mode = CodexAccessMode::Relay;
        settings.api_endpoint = Some("https://api.example.com/v1".into());
        settings.api_key = Some("sk-test".into());
        settings.api_site_name = "示例站".into();
        settings.api_model = "gpt-5.4".into();
        settings.reasoning_effort = ReasoningEffort::Extreme;
        settings.speed_mode = ApiSpeedMode::Fast;
        settings.unify_codex_session_history = true;

        sync_test(&settings, &config_path, &auth_path, &restore_path).unwrap();

        let text = fs::read_to_string(&config_path).unwrap();
        assert!(text.contains(r#"model = "gpt-5.4""#));
        assert!(text.contains(r#"model_provider = "qianzong_unified""#));
        assert!(text.contains(r#"preferred_auth_method = "apikey""#));
        assert!(text.contains(r#"model_reasoning_effort = "xhigh""#));
        assert!(text.contains(r#"service_tier = "priority""#));
        assert!(text.contains(r#"[model_providers.qianzong_unified]"#));
        assert!(text.contains(r#"name = "API：示例站""#));
        assert!(text.contains(r#"base_url = "https://api.example.com/v1""#));
        assert!(text.contains(r#"wire_api = "responses""#));

        let restore = fs::read_to_string(&restore_path).unwrap();
        assert!(restore.contains(r#"preferred_auth_method = "chatgpt""#));
        let auth = fs::read_to_string(&auth_path).unwrap();
        assert!(auth.contains(r#""auth_mode": "apikey""#));
        assert!(auth.contains(r#""OPENAI_API_KEY": "sk-test""#));
        assert!(temp.path().read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("qianzong-backup")));
    }

    #[test]
    fn relay_config_omits_empty_site_name_and_removes_old_name() {
        let mut doc = parse_config(
            r#"model_provider = "qianzong_relay"

[model_providers.qianzong_relay]
name = "API：旧站点"
base_url = "https://old.example.com/v1"
wire_api = "responses"
"#,
        )
        .unwrap();
        let settings = AppSettings {
            access_mode: CodexAccessMode::Relay,
            api_endpoint: Some("https://api.example.com/v1".into()),
            ..AppSettings::default()
        };

        apply_relay_config(&mut doc, &settings).unwrap();

        let provider = doc["model_providers"][LEGACY_RELAY_PROVIDER_ID]
            .as_table()
            .unwrap();
        assert!(provider.get("name").is_none());
        assert_eq!(
            provider.get("base_url").and_then(Item::as_str),
            Some("https://api.example.com/v1")
        );
    }

    #[test]
    fn official_sync_restores_official_provider_shape() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let auth_path = temp.path().join("auth.json");
        let restore_path = temp.path().join("codex-config-restore.toml");
        fs::write(
            &config_path,
            r#"model = "relay-model"
model_provider = "qianzong_relay"
preferred_auth_method = "apikey"
model_reasoning_effort = "xhigh"
service_tier = "priority"

[model_providers.qianzong_relay]
name = "qianzong_relay"
base_url = "https://api.example.com/v1"
wire_api = "responses"

[mcp_servers.current]
command = "node"

[projects."/Users/mac/project-a"]
trust_level = "trusted"
"#,
        )
        .unwrap();
        fs::write(
            &restore_path,
            r#"model = "gpt-5.4"
preferred_auth_method = "chatgpt"
service_tier = "priority"

[mcp_servers.stale_restore]
command = "stale"
"#,
        )
        .unwrap();
        fs::write(
            &auth_path,
            r#"{
  "auth_mode": "apikey",
  "OPENAI_API_KEY": "sk-test",
  "tokens": { "access_token": "access", "refresh_token": "refresh" }
}
"#,
        )
        .unwrap();

        let settings = AppSettings {
            unify_codex_session_history: true,
            ..AppSettings::default()
        };
        sync_test(&settings, &config_path, &auth_path, &restore_path).unwrap();

        let text = fs::read_to_string(&config_path).unwrap();
        assert!(text.contains(r#"model = "gpt-5.5""#));
        assert!(text.contains(r#"preferred_auth_method = "chatgpt""#));
        assert!(text.contains(r#"model_reasoning_effort = "medium""#));
        assert!(text.contains(r#"[mcp_servers.current]"#));
        assert!(text.contains(r#"[projects."/Users/mac/project-a"]"#));
        assert!(text.contains(r#"trust_level = "trusted""#));
        assert!(!text.contains("stale_restore"));
        assert!(!text.contains("service_tier"));
        assert!(!text.contains("qianzong_relay"));
        assert!(text.contains(r#"model_provider = "qianzong_unified""#));
        assert!(text.contains(r#"[model_providers.qianzong_unified]"#));
        assert!(text.contains(r#"requires_openai_auth = true"#));
        let auth = fs::read_to_string(&auth_path).unwrap();
        assert!(auth.contains(r#""auth_mode": "chatgpt""#));
        assert!(auth.contains(r#""OPENAI_API_KEY": null"#));
    }

    #[test]
    fn official_sync_repairs_apikey_auth_residue_without_restore_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let auth_path = temp.path().join("auth.json");
        let restore_path = temp.path().join("codex-config-restore.toml");
        fs::write(
            &config_path,
            r#"model = "gpt-5.5"
preferred_auth_method = "apikey"
service_tier = "priority"
"#,
        )
        .unwrap();
        fs::write(
            &auth_path,
            r#"{
  "auth_mode": "chatgpt",
  "OPENAI_API_KEY": null,
  "tokens": {}
}
"#,
        )
        .unwrap();

        let settings = AppSettings::default();
        sync_test(&settings, &config_path, &auth_path, &restore_path).unwrap();

        let text = fs::read_to_string(&config_path).unwrap();
        assert!(text.contains(r#"preferred_auth_method = "chatgpt""#));
        assert!(!text.contains(r#"preferred_auth_method = "apikey""#));
        assert!(!text.contains("service_tier"));
        let auth = fs::read_to_string(&auth_path).unwrap();
        assert!(auth.contains(r#""auth_mode": "chatgpt""#));
    }

    #[test]
    fn official_sync_without_saved_login_returns_to_login_ready_state() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let auth_path = temp.path().join("auth.json");
        let restore_path = temp.path().join("restore.toml");
        fs::write(
            &config_path,
            r#"model_provider = "qianzong_relay"
preferred_auth_method = "apikey"

[model_providers.qianzong_relay]
base_url = "https://api.example.com/v1"
wire_api = "responses"
"#,
        )
        .unwrap();
        fs::write(
            &auth_path,
            r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-relay"}"#,
        )
        .unwrap();

        let vault = MemoryAuthVault::default();
        let login_ready_settings = AppSettings {
            unify_codex_session_history: true,
            ..AppSettings::default()
        };
        sync_codex_config_for_paths(
            &login_ready_settings,
            &config_path,
            &auth_path,
            &restore_path,
            &vault,
            None,
        )
        .unwrap();

        let config = fs::read_to_string(config_path).unwrap();
        assert!(config.contains(r#"model_provider = "openai""#));
        assert!(config.contains(r#"preferred_auth_method = "chatgpt""#));
        assert!(!config.contains("qianzong_relay"));
        assert!(!config.contains("qianzong_unified"));
        assert_eq!(fs::read_to_string(auth_path).unwrap(), "{}\n");
        assert!(!temp.path().read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("auth.json.qianzong-backup-")));

        let relay = AppSettings {
            access_mode: CodexAccessMode::Relay,
            api_endpoint: Some("https://api.example.com/v1".into()),
            api_key: None,
            ..AppSettings::default()
        };
        sync_codex_config_for_paths(
            &relay,
            &temp.path().join("config.toml"),
            &temp.path().join("auth.json"),
            &restore_path,
            &vault,
            None,
        )
        .unwrap();
        assert!(fs::read_to_string(temp.path().join("auth.json"))
            .unwrap()
            .contains(r#""OPENAI_API_KEY": "sk-relay""#));
    }

    #[test]
    fn relay_sync_requires_endpoint() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = AppSettings::default();
        settings.access_mode = CodexAccessMode::Relay;
        settings.api_endpoint = None;

        let err = sync_test(
            &settings,
            &temp.path().join("config.toml"),
            &temp.path().join("auth.json"),
            &temp.path().join("restore.toml"),
        )
        .unwrap_err();

        assert!(err.to_string().contains("API 中转模式需要填写 API 地址"));
    }

    #[test]
    fn relay_sync_requires_api_key_when_auth_has_no_existing_key() {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = AppSettings::default();
        settings.access_mode = CodexAccessMode::Relay;
        settings.api_endpoint = Some("https://api.example.com/v1".into());

        let err = sync_test(
            &settings,
            &temp.path().join("config.toml"),
            &temp.path().join("auth.json"),
            &temp.path().join("restore.toml"),
        )
        .unwrap_err();

        assert!(err.to_string().contains("API 中转模式需要填写 API Key"));
    }

    #[test]
    fn relay_sync_preserves_existing_api_key_when_input_is_empty() {
        let temp = tempfile::tempdir().unwrap();
        let auth_path = temp.path().join("auth.json");
        fs::write(
            temp.path().join("config.toml"),
            r#"model_provider = "qianzong_relay"
[model_providers.qianzong_relay]
base_url = "https://api.example.com/v1"
"#,
        )
        .unwrap();
        fs::write(
            &auth_path,
            r#"{
  "auth_mode": "apikey",
  "OPENAI_API_KEY": "sk-existing"
}
"#,
        )
        .unwrap();
        let mut settings = AppSettings::default();
        settings.access_mode = CodexAccessMode::Relay;
        settings.api_endpoint = Some("https://api.example.com/v1".into());

        sync_test(
            &settings,
            &temp.path().join("config.toml"),
            &auth_path,
            &temp.path().join("restore.toml"),
        )
        .unwrap();

        let auth = fs::read_to_string(&auth_path).unwrap();
        assert!(auth.contains(r#""OPENAI_API_KEY": "sk-existing""#));
    }

    #[test]
    fn relay_sync_requires_new_key_when_endpoint_changes() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let auth_path = temp.path().join("auth.json");
        let restore_path = temp.path().join("restore.toml");
        let vault = MemoryAuthVault::default();
        let first = AppSettings {
            access_mode: CodexAccessMode::Relay,
            api_endpoint: Some("https://first.example.com/v1".into()),
            api_key: Some("sk-first".into()),
            ..AppSettings::default()
        };
        sync_codex_config_for_paths(
            &first,
            &config_path,
            &auth_path,
            &restore_path,
            &vault,
            None,
        )
        .unwrap();

        let changed = AppSettings {
            api_endpoint: Some("https://second.example.com/v1".into()),
            api_key: None,
            ..first
        };
        let err = sync_codex_config_for_paths(
            &changed,
            &config_path,
            &auth_path,
            &restore_path,
            &vault,
            None,
        )
        .unwrap_err();

        assert!(err.to_string().contains("API 地址已变化"));
        assert_eq!(
            vault.snapshot().relay.unwrap().endpoint,
            "https://first.example.com/v1"
        );
    }

    #[test]
    fn official_and_relay_credentials_round_trip_through_vault() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("config.toml");
        let auth_path = temp.path().join("auth.json");
        let restore_path = temp.path().join("restore.toml");
        fs::write(&config_path, "model = \"gpt-5.5\"\n").unwrap();
        fs::write(
            &auth_path,
            r#"{
  "auth_mode": "chatgpt",
  "OPENAI_API_KEY": null,
  "tokens": { "access_token": "access", "refresh_token": "refresh" },
  "account_id": "acct"
}
"#,
        )
        .unwrap();
        let vault = MemoryAuthVault::default();
        let relay = AppSettings {
            access_mode: CodexAccessMode::Relay,
            api_endpoint: Some("https://api.example.com/v1".into()),
            api_key: Some("sk-relay".into()),
            unify_codex_session_history: true,
            ..AppSettings::default()
        };
        sync_codex_config_for_paths(
            &relay,
            &config_path,
            &auth_path,
            &restore_path,
            &vault,
            None,
        )
        .unwrap();
        let relay_auth = fs::read_to_string(&auth_path).unwrap();
        assert!(relay_auth.contains(r#""OPENAI_API_KEY": "sk-relay""#));
        assert!(!relay_auth.contains("refresh_token"));
        assert!(vault.snapshot().status().has_stored_official_auth);
        assert!(vault.snapshot().status().has_stored_relay_api_key);

        let official = AppSettings {
            access_mode: CodexAccessMode::Official,
            unify_codex_session_history: true,
            ..relay
        };
        sync_codex_config_for_paths(
            &official,
            &config_path,
            &auth_path,
            &restore_path,
            &vault,
            None,
        )
        .unwrap();
        let official_auth = fs::read_to_string(&auth_path).unwrap();
        assert!(official_auth.contains(r#""auth_mode": "chatgpt""#));
        assert!(official_auth.contains(r#""refresh_token": "refresh""#));
        assert!(official_auth.contains(r#""account_id": "acct""#));
        assert!(official_auth.contains(r#""OPENAI_API_KEY": null"#));
    }

    #[test]
    fn official_mode_recovers_auth_from_backup_when_vault_is_empty() {
        let vault = MemoryAuthVault::default();
        let fallback = serde_json::json!({
            "auth_mode": "chatgpt",
            "tokens": {
                "access_token": "access",
                "refresh_token": "refresh",
                "account_id": "acct"
            },
            "last_refresh": "2026-07-16T00:00:00Z"
        })
        .as_object()
        .unwrap()
        .clone();
        let settings = AppSettings {
            access_mode: CodexAccessMode::Official,
            ..AppSettings::default()
        };

        let restored = prepare_codex_auth(
            &settings,
            r#"model_provider = "qianzong_relay""#,
            r#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-relay"}"#,
            &vault,
            Some(&fallback),
        )
        .unwrap();

        assert_eq!(
            restored.get("auth_mode").and_then(Value::as_str),
            Some("chatgpt")
        );
        assert_eq!(
            Value::Object(restored.clone())
                .pointer("/tokens/refresh_token")
                .and_then(Value::as_str),
            Some("refresh")
        );
        assert!(vault.snapshot().status().has_stored_official_auth);
    }

    #[test]
    fn restore_entry_removes_current_file_when_backup_recorded_absence() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("config.toml");
        fs::write(&target, "model = \"relay\"\n").unwrap();

        restore_snapshot_entry(
            &temp.path().join("missing.toml"),
            &target,
            "config.toml",
            false,
        )
        .unwrap();

        assert!(!target.exists());
        assert!(temp.path().read_dir().unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("qianzong-backup")));
    }

    #[test]
    fn backup_id_validation_rejects_path_traversal() {
        let err = backup_dir_for_id(Path::new("/tmp/.codex"), "../manual").unwrap_err();
        assert!(err.to_string().contains("备份 ID 不合法"));
    }

    #[test]
    fn qianzong_detection_handles_toml_quoting_variants() {
        assert!(is_qianzong_managed("model_provider = 'qianzong_relay'"));
        assert!(is_qianzong_managed(
            "[model_providers.qianzong_relay]\nbase_url = 'https://api.example.com/v1'"
        ));
    }
}
