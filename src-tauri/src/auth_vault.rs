use crate::{
    atomic_file,
    error::{AppError, AppResult},
    paths,
};
use chacha20poly1305::{
    aead::{Aead, AeadCore, OsRng},
    ChaCha20Poly1305, Key, KeyInit, Nonce,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

const VAULT_VERSION: u32 = 1;
const VAULT_MAGIC: &[u8; 8] = b"CQAUTH01";
#[cfg(not(windows))]
const LOCAL_MASTER_KEY_FILE: &str = "auth-vault.key";
#[cfg(windows)]
const KEYRING_SERVICE: &str = "com.qianzong.codex";
#[cfg(windows)]
const KEYRING_ACCOUNT: &str = "auth-vault-master-key";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AuthVaultData {
    #[serde(default = "vault_version")]
    pub version: u32,
    pub official_auth: Option<Value>,
    pub relay: Option<RelayCredential>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelayCredential {
    pub endpoint: String,
    pub api_key: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AuthCredentialStatus {
    pub has_stored_official_auth: bool,
    pub has_stored_relay_api_key: bool,
    pub relay_endpoint: Option<String>,
}

pub trait AuthVaultStore: Send + Sync {
    fn load(&self) -> AppResult<AuthVaultData>;
    fn save(&self, data: &AuthVaultData) -> AppResult<()>;
}

#[cfg(test)]
#[derive(Debug, Default)]
pub struct MemoryAuthVault(std::sync::Mutex<AuthVaultData>);

#[cfg(test)]
impl MemoryAuthVault {
    pub fn snapshot(&self) -> AuthVaultData {
        self.0.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl AuthVaultStore for MemoryAuthVault {
    fn load(&self) -> AppResult<AuthVaultData> {
        Ok(self.snapshot())
    }

    fn save(&self, data: &AuthVaultData) -> AppResult<()> {
        *self.0.lock().unwrap() = data.clone();
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemAuthVault;

impl SystemAuthVault {
    pub fn status(self) -> AppResult<AuthCredentialStatus> {
        Ok(self.load()?.status())
    }

    pub fn clear_relay(self) -> AppResult<AuthCredentialStatus> {
        let mut data = self.load()?;
        data.relay = None;
        self.save(&data)?;
        Ok(data.status())
    }
}

impl AuthVaultStore for SystemAuthVault {
    fn load(&self) -> AppResult<AuthVaultData> {
        let path = vault_path()?;
        if !path.exists() {
            return Ok(AuthVaultData::default());
        }
        let encrypted = fs::read(&path)?;
        let Some(key) = load_master_key(false)? else {
            #[cfg(not(windows))]
            return Ok(AuthVaultData::default());
            #[cfg(windows)]
            return Err(AppError::Config(
                "认证保险箱主密钥缺失，无法读取已保存凭据".to_string(),
            ));
        };
        decrypt_vault(&encrypted, &key)
    }

    fn save(&self, data: &AuthVaultData) -> AppResult<()> {
        let path = vault_path()?;
        let existing_key = load_master_key(false)?;
        #[cfg(not(windows))]
        if existing_key.is_none() && path.exists() {
            backup_legacy_vault(&path)?;
        }
        let key = match existing_key {
            Some(key) => key,
            None => load_master_key(true)?
                .ok_or_else(|| AppError::Config("无法创建认证保险箱主密钥".to_string()))?,
        };
        let encrypted = encrypt_vault(data, &key)?;
        atomic_write(&path, &encrypted)
    }
}

impl Default for AuthVaultData {
    fn default() -> Self {
        Self {
            version: VAULT_VERSION,
            official_auth: None,
            relay: None,
        }
    }
}

impl AuthVaultData {
    pub fn status(&self) -> AuthCredentialStatus {
        AuthCredentialStatus {
            has_stored_official_auth: self
                .official_auth
                .as_ref()
                .is_some_and(has_official_login_material),
            has_stored_relay_api_key: self
                .relay
                .as_ref()
                .is_some_and(|relay| !relay.api_key.trim().is_empty()),
            relay_endpoint: self.relay.as_ref().map(|relay| relay.endpoint.clone()),
        }
    }

    pub fn capture_official_auth(&mut self, auth: &Map<String, Value>) -> bool {
        if !has_official_login_material(&Value::Object(auth.clone())) {
            return false;
        }
        let mut snapshot = auth.clone();
        snapshot.insert(
            "auth_mode".to_string(),
            Value::String("chatgpt".to_string()),
        );
        snapshot.insert("OPENAI_API_KEY".to_string(), Value::Null);
        let next = Value::Object(snapshot);
        if self.official_auth.as_ref() == Some(&next) {
            return false;
        }
        self.official_auth = Some(next);
        true
    }

    pub fn official_auth_map(&self) -> Option<Map<String, Value>> {
        self.official_auth.as_ref()?.as_object().cloned()
    }
}

fn vault_version() -> u32 {
    VAULT_VERSION
}

fn has_official_login_material(value: &Value) -> bool {
    let Some(auth) = value.as_object() else {
        return false;
    };
    const TOKEN_FIELDS: [&str; 3] = ["access_token", "refresh_token", "id_token"];
    let has_value = |value: Option<&Value>| {
        value
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    };

    TOKEN_FIELDS.iter().any(|field| has_value(auth.get(*field)))
        || auth
            .get("tokens")
            .and_then(Value::as_object)
            .is_some_and(|tokens| {
                TOKEN_FIELDS
                    .iter()
                    .any(|field| has_value(tokens.get(*field)))
            })
}

fn vault_path() -> AppResult<PathBuf> {
    Ok(vault_dir()?.join("auth-vault.bin"))
}

fn vault_dir() -> AppResult<PathBuf> {
    Ok(paths::app_log_dir()?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".")))
}

#[cfg(windows)]
fn load_master_key(create: bool) -> AppResult<Option<[u8; 32]>> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|err| AppError::Config(format!("初始化系统凭据库失败: {err}")))?;
    match entry.get_password() {
        Ok(value) => return decode_key(&value).map(Some),
        Err(keyring::Error::NoEntry) if !create => return Ok(None),
        Err(keyring::Error::NoEntry) => {}
        Err(err) => return Err(AppError::Config(format!("读取系统凭据库失败: {err}"))),
    }

    let key = ChaCha20Poly1305::generate_key(&mut OsRng);
    let encoded = encode_key(key.as_slice());
    entry
        .set_password(&encoded)
        .map_err(|err| AppError::Config(format!("保存认证保险箱主密钥失败: {err}")))?;
    let mut result = [0_u8; 32];
    result.copy_from_slice(key.as_slice());
    Ok(Some(result))
}

#[cfg(not(windows))]
fn load_master_key(create: bool) -> AppResult<Option<[u8; 32]>> {
    load_file_master_key(&vault_dir()?.join(LOCAL_MASTER_KEY_FILE), create)
}

#[cfg(not(windows))]
fn load_file_master_key(path: &Path, create: bool) -> AppResult<Option<[u8; 32]>> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

    if path.exists() {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
        let bytes = fs::read(path)?;
        if bytes.len() != 32 {
            return Err(AppError::Config("认证保险箱本地主密钥格式无效".to_string()));
        }
        let mut key = [0_u8; 32];
        key.copy_from_slice(&bytes);
        return Ok(Some(key));
    }
    if !create {
        return Ok(None);
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let key = ChaCha20Poly1305::generate_key(&mut OsRng);
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            return load_file_master_key(path, false);
        }
        Err(err) => return Err(err.into()),
    };
    file.write_all(key.as_slice())?;
    file.sync_all()?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;

    let mut result = [0_u8; 32];
    result.copy_from_slice(key.as_slice());
    Ok(Some(result))
}

#[cfg(not(windows))]
fn backup_legacy_vault(path: &Path) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let backup = path.with_file_name(format!("auth-vault.keychain-backup-{timestamp}.bin"));
    fs::copy(path, &backup)?;
    fs::set_permissions(backup, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(windows)]
fn encode_key(key: &[u8]) -> String {
    key.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(windows)]
fn decode_key(value: &str) -> AppResult<[u8; 32]> {
    if value.len() != 64 {
        return Err(AppError::Config("认证保险箱主密钥格式无效".to_string()));
    }
    let mut key = [0_u8; 32];
    for (index, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(chunk)
            .map_err(|_| AppError::Config("认证保险箱主密钥编码无效".to_string()))?;
        key[index] = u8::from_str_radix(text, 16)
            .map_err(|_| AppError::Config("认证保险箱主密钥编码无效".to_string()))?;
    }
    Ok(key)
}

fn encrypt_vault(data: &AuthVaultData, key: &[u8; 32]) -> AppResult<Vec<u8>> {
    let plaintext = serde_json::to_vec(data)?;
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|_| AppError::Config("加密认证保险箱失败".to_string()))?;
    let mut output = Vec::with_capacity(VAULT_MAGIC.len() + nonce.len() + ciphertext.len());
    output.extend_from_slice(VAULT_MAGIC);
    output.extend_from_slice(&nonce);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

fn decrypt_vault(bytes: &[u8], key: &[u8; 32]) -> AppResult<AuthVaultData> {
    let header_len = VAULT_MAGIC.len() + 12;
    if bytes.len() <= header_len || &bytes[..VAULT_MAGIC.len()] != VAULT_MAGIC {
        return Err(AppError::Config("认证保险箱文件格式无效".to_string()));
    }
    let nonce = Nonce::from_slice(&bytes[VAULT_MAGIC.len()..header_len]);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let plaintext = cipher
        .decrypt(nonce, &bytes[header_len..])
        .map_err(|_| AppError::Config("认证保险箱解密失败".to_string()))?;
    let data: AuthVaultData = serde_json::from_slice(&plaintext)?;
    if data.version != VAULT_VERSION {
        return Err(AppError::Config(format!(
            "不支持的认证保险箱版本: {}",
            data.version
        )));
    }
    Ok(data)
}

fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> AppResult<()> {
    atomic_file::write(path, bytes)?;
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_round_trip_keeps_full_official_auth_and_relay_key() {
        let key = [7_u8; 32];
        let data = AuthVaultData {
            version: VAULT_VERSION,
            official_auth: Some(serde_json::json!({
                "auth_mode": "chatgpt",
                "tokens": { "access_token": "access", "refresh_token": "refresh" },
                "account_id": "acct"
            })),
            relay: Some(RelayCredential {
                endpoint: "https://api.example.com/v1".to_string(),
                api_key: "sk-secret".to_string(),
            }),
        };

        let encrypted = encrypt_vault(&data, &key).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("sk-secret"));
        assert_eq!(decrypt_vault(&encrypted, &key).unwrap(), data);
    }

    #[test]
    fn official_capture_preserves_unknown_fields_but_clears_relay_key() {
        let mut data = AuthVaultData::default();
        let auth = serde_json::json!({
            "auth_mode": "apikey",
            "OPENAI_API_KEY": "sk-relay",
            "tokens": { "refresh_token": "refresh" },
            "future_field": { "kept": true }
        })
        .as_object()
        .unwrap()
        .clone();

        assert!(data.capture_official_auth(&auth));
        let stored = data.official_auth_map().unwrap();
        assert_eq!(stored.get("OPENAI_API_KEY"), Some(&Value::Null));
        assert_eq!(
            Value::Object(stored).pointer("/future_field/kept"),
            Some(&Value::Bool(true))
        );
    }

    #[test]
    fn empty_or_blank_tokens_are_not_official_login_material() {
        for auth in [
            serde_json::json!({ "tokens": {} }),
            serde_json::json!({ "tokens": { "access_token": "  " } }),
            serde_json::json!({ "access_token": "" }),
        ] {
            assert!(!has_official_login_material(&auth));
        }
        assert!(has_official_login_material(&serde_json::json!({
            "tokens": { "refresh_token": "refresh" }
        })));
    }

    #[cfg(not(windows))]
    #[test]
    fn local_master_key_is_stable_and_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join(LOCAL_MASTER_KEY_FILE);
        let created = load_file_master_key(&path, true).unwrap().unwrap();
        let loaded = load_file_master_key(&path, false).unwrap().unwrap();

        assert_eq!(created, loaded);
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
