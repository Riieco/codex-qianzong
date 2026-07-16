use crate::{
    auth_vault::{AuthVaultStore, SystemAuthVault},
    error::{AppError, AppResult},
};
use reqwest::{header, redirect::Policy, Client, Url};
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};

const MAX_MODELS_RESPONSE_BYTES: u64 = 4 * 1024 * 1024;

pub async fn fetch_openai_models(
    api_endpoint: &str,
    provided_api_key: Option<&str>,
) -> AppResult<Vec<String>> {
    let endpoint = normalize_api_endpoint(api_endpoint)?;
    let api_key = resolve_api_key(&endpoint, provided_api_key)?;
    let url = models_url(&endpoint)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(Policy::none())
        .build()
        .map_err(|err| AppError::Process(format!("创建模型请求失败: {err}")))?;
    let response = client
        .get(url)
        .header(header::ACCEPT, "application/json")
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|err| AppError::Process(format!("获取模型列表失败: {err}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(AppError::Process(format!(
            "获取模型列表失败，API 返回 HTTP {}",
            status.as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_MODELS_RESPONSE_BYTES)
    {
        return Err(AppError::Process("模型列表响应过大".to_string()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|err| AppError::Process(format!("读取模型列表失败: {err}")))?;
    if bytes.len() as u64 > MAX_MODELS_RESPONSE_BYTES {
        return Err(AppError::Process("模型列表响应过大".to_string()));
    }
    let payload: Value = serde_json::from_slice(&bytes)
        .map_err(|err| AppError::Config(format!("模型列表不是有效 JSON: {err}")))?;
    parse_openai_models(&payload)
}

fn resolve_api_key(endpoint: &str, provided_api_key: Option<&str>) -> AppResult<String> {
    if let Some(api_key) = provided_api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Ok(api_key.to_string());
    }
    let vault = SystemAuthVault.load()?;
    let relay = vault.relay.ok_or_else(|| {
        AppError::Config("请先填写 API Key，或保存当前 API 地址对应的 Key".to_string())
    })?;
    let stored_endpoint = normalize_api_endpoint(&relay.endpoint)?;
    if stored_endpoint != endpoint {
        return Err(AppError::Config(
            "API 地址与保险箱中保存的 Key 不匹配，请重新输入 API Key".to_string(),
        ));
    }
    Ok(relay.api_key)
}

fn models_url(endpoint: &str) -> AppResult<Url> {
    Url::parse(&format!("{endpoint}/models"))
        .map_err(|err| AppError::Config(format!("API 地址无效: {err}")))
}

fn normalize_api_endpoint(value: &str) -> AppResult<String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err(AppError::Config("请先填写 API 地址".to_string()));
    }
    let with_scheme = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    let mut base = with_scheme.trim_end_matches('/').to_string();
    while base.to_ascii_lowercase().ends_with("/v1/v1") {
        base.truncate(base.len() - 3);
    }
    if !base.to_ascii_lowercase().ends_with("/v1") {
        base.push_str("/v1");
    }
    let parsed =
        Url::parse(&base).map_err(|err| AppError::Config(format!("API 地址无效: {err}")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(AppError::Config(
            "API 地址必须是有效的 HTTP(S) 地址".to_string(),
        ));
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(AppError::Config(
            "API 地址不能包含账号、查询参数或片段".to_string(),
        ));
    }
    Ok(base)
}

fn parse_openai_models(payload: &Value) -> AppResult<Vec<String>> {
    let data = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::Config("模型列表响应缺少 data 数组".to_string()))?;
    let mut models = BTreeSet::new();
    for item in data {
        let (id, owned_by) = match item {
            Value::String(id) => (id.as_str(), None),
            Value::Object(model) => (
                model.get("id").and_then(Value::as_str).unwrap_or_default(),
                model.get("owned_by").and_then(Value::as_str),
            ),
            _ => continue,
        };
        if is_openai_text_model(id, owned_by) {
            models.insert(id.to_string());
        }
    }
    if models.is_empty() {
        return Err(AppError::Config(
            "接口返回了模型，但没有找到可用于 Codex 的 OpenAI 模型".to_string(),
        ));
    }
    Ok(models.into_iter().collect())
}

fn is_openai_text_model(id: &str, owned_by: Option<&str>) -> bool {
    let id = id.trim().to_ascii_lowercase();
    if id.is_empty() {
        return false;
    }
    let owner = owned_by.unwrap_or_default().trim().to_ascii_lowercase();
    if [
        "anthropic",
        "google",
        "gemini",
        "meta",
        "mistral",
        "cohere",
        "deepseek",
        "qwen",
        "alibaba",
        "xai",
    ]
    .iter()
    .any(|provider| owner.contains(provider))
    {
        return false;
    }

    let canonical = id
        .strip_prefix("openai/")
        .or_else(|| id.strip_prefix("openai:"))
        .unwrap_or(&id);
    if [
        "moderation",
        "safety",
        "guard",
        "review",
        "dall-e",
        "gpt-image",
        "image-generation",
        "embedding",
        "whisper",
        "transcription",
        "tts",
        "realtime",
        "audio",
    ]
    .iter()
    .any(|blocked| canonical.contains(blocked))
    {
        return false;
    }

    canonical.starts_with("gpt-")
        || canonical.starts_with("chatgpt-")
        || canonical.starts_with("codex-")
        || is_reasoning_family(canonical)
}

fn is_reasoning_family(id: &str) -> bool {
    let Some(rest) = id.strip_prefix('o') else {
        return false;
    };
    let digit_count = rest.chars().take_while(|ch| ch.is_ascii_digit()).count();
    let suffix = &rest[digit_count..];
    digit_count > 0 && (suffix.is_empty() || suffix.starts_with('-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_newapi_model_list_to_openai_codex_models() {
        let payload = serde_json::json!({
            "object": "list",
            "data": [
                { "id": "gpt-5", "owned_by": "openai" },
                { "id": "gpt-4o", "owned_by": "system" },
                { "id": "o3-mini", "owned_by": "openai" },
                { "id": "codex-mini-latest", "owned_by": "openai" },
                { "id": "openai/gpt-5-codex", "owned_by": "new-api" },
                { "id": "claude-3-7-sonnet", "owned_by": "anthropic" },
                { "id": "gemini-2.5-pro", "owned_by": "google" },
                { "id": "gpt-image-1", "owned_by": "openai" },
                { "id": "omni-moderation-latest", "owned_by": "openai" },
                { "id": "text-embedding-3-large", "owned_by": "openai" },
                { "id": "gpt-4o-realtime-preview", "owned_by": "openai" }
            ]
        });

        assert_eq!(
            parse_openai_models(&payload).unwrap(),
            vec![
                "codex-mini-latest",
                "gpt-4o",
                "gpt-5",
                "o3-mini",
                "openai/gpt-5-codex"
            ]
        );
    }

    #[test]
    fn rejects_non_openai_owner_even_with_openai_like_alias() {
        assert!(!is_openai_text_model("gpt-5-alias", Some("anthropic")));
        assert!(is_openai_text_model("chatgpt-4o-latest", Some("system")));
        assert!(is_openai_text_model("o5-mini", Some("openai")));
    }

    #[test]
    fn normalizes_base_url_to_single_v1() {
        assert_eq!(
            normalize_api_endpoint("api.example.com/v1/v1/").unwrap(),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn rejects_endpoint_query_parameters() {
        assert!(
            normalize_api_endpoint("https://api.example.com/v1?target=other")
                .unwrap_err()
                .to_string()
                .contains("查询参数")
        );
    }
}
