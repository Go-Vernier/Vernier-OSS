//! `--explain`: the blast radius in plain words, written by an LLM the user
//! picks and pays for. Off unless asked for. Keys come from the
//! environment, never from flags. The model is sent the blast radius only:
//! service names, changed file paths, the hops between services and their
//! confidence. No source code and no evidence snippets, which can hold
//! connection strings.
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

use crate::blast::Blast;

pub const TIMEOUT_SECONDS: u64 = 120;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ExplainError {
    #[error("{0}")]
    Config(String),
    #[error("{0}")]
    Http(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    Anthropic,
    Openai,
    Gemini,
    Ollama,
    OpenaiCompatible,
}

pub const PROVIDERS: &str = "anthropic, openai, gemini, ollama, openai-compatible";

impl Provider {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Some(Self::Anthropic),
            "openai" => Some(Self::Openai),
            "gemini" | "google" => Some(Self::Gemini),
            "ollama" => Some(Self::Ollama),
            "openai-compatible" | "compatible" => Some(Self::OpenaiCompatible),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::Openai => "openai",
            Self::Gemini => "gemini",
            Self::Ollama => "ollama",
            Self::OpenaiCompatible => "openai-compatible",
        }
    }

    /// The variable holding the key. Ollama needs none; an OpenAI-compatible
    /// server may or may not.
    pub fn key_var(self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::Openai => "OPENAI_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
            Self::Ollama | Self::OpenaiCompatible => "VERNIER_LLM_API_KEY",
        }
    }

    fn key_required(self) -> bool {
        matches!(self, Self::Anthropic | Self::Openai | Self::Gemini)
    }

    /// Local and self-hosted servers run whatever was pulled, so there is no
    /// default to guess.
    pub fn default_model(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("claude-opus-5-5"),
            Self::Openai => Some("gpt-6.1-sol"),
            Self::Gemini => Some("gemini-3.8-flash"),
            Self::Ollama | Self::OpenaiCompatible => None,
        }
    }

    pub fn default_url(self) -> Option<&'static str> {
        match self {
            Self::Anthropic => Some("https://api.anthropic.com/v1"),
            Self::Openai => Some("https://api.openai.com/v1"),
            Self::Gemini => Some("https://generativelanguage.googleapis.com/v1beta/openai"),
            Self::Ollama => Some("http://localhost:11434/v1"),
            Self::OpenaiCompatible => None,
        }
    }
}

/// Which provider, model, endpoint and key one explanation uses.
#[derive(Clone, PartialEq, Eq)]
pub struct LlmConfig {
    pub provider: Provider,
    pub model: String,
    /// The API base, without the trailing path: `https://api.openai.com/v1`.
    pub url: String,
    pub key: Option<String>,
}

impl std::fmt::Debug for LlmConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LlmConfig")
            .field("provider", &self.provider)
            .field("model", &self.model)
            .field("url", &self.url)
            .field("key", &self.key.as_ref().map(|_| "<set>"))
            .finish()
    }
}

/// Picks the provider from the flag, else `VERNIER_LLM`, else the first
/// provider key found in the environment; the model from the flag, else
/// `VERNIER_LLM_MODEL`, else the provider's default; the endpoint from
/// `VERNIER_LLM_URL`, else the provider's. `env` reads one variable, so
/// tests never touch the process environment.
pub fn resolve(
    provider: Option<&str>,
    model: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<LlmConfig, ExplainError> {
    let var = |name: &str| env(name).filter(|v| !v.trim().is_empty());
    let named = provider.map(str::to_string).or_else(|| var("VERNIER_LLM"));
    let provider = match named {
        Some(name) => Provider::parse(&name).ok_or_else(|| {
            ExplainError::Config(format!(
                "unknown LLM provider {name:?}; use one of: {PROVIDERS}"
            ))
        })?,
        None => [Provider::Anthropic, Provider::Openai, Provider::Gemini]
            .into_iter()
            .find(|p| var(p.key_var()).is_some())
            .ok_or_else(|| {
                ExplainError::Config(format!(
                    "--explain needs an LLM: set ANTHROPIC_API_KEY, OPENAI_API_KEY or GEMINI_API_KEY, or pick one with --llm <{}>",
                    PROVIDERS.replace(", ", "|")
                ))
            })?,
    };
    let model = model
        .map(str::to_string)
        .or_else(|| var("VERNIER_LLM_MODEL"))
        .or_else(|| provider.default_model().map(str::to_string))
        .ok_or_else(|| {
            ExplainError::Config(format!(
                "{} has no default model; pass --model <name>{}",
                provider.as_str(),
                if provider == Provider::Ollama {
                    " (see `ollama list`)"
                } else {
                    ""
                }
            ))
        })?;
    let url = var("VERNIER_LLM_URL")
        .or_else(|| provider.default_url().map(str::to_string))
        .ok_or_else(|| {
            ExplainError::Config(
                "openai-compatible needs VERNIER_LLM_URL, the API base such as http://localhost:8000/v1".into(),
            )
        })?
        .trim_end_matches('/')
        .to_string();
    let key = var(provider.key_var());
    if key.is_none() && provider.key_required() {
        return Err(ExplainError::Config(format!(
            "--explain with {} needs {} in the environment",
            provider.as_str(),
            provider.key_var()
        )));
    }
    Ok(LlmConfig {
        provider,
        model,
        url,
        key,
    })
}

/// The explanation as printed and as the JSON carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    pub provider: Provider,
    pub model: String,
    pub text: String,
}

pub const SYSTEM: &str = "You explain the blast radius of a code change to the engineer reviewing it. \
The input is JSON from Vernier, a static analysis tool: the files changed, the services that own them, \
and the services the change can reach. Each reached service carries its path of hops (calls, consumes \
events, shares a database, imports) and a confidence: observed (seen in production traces), static \
(found in the code), inferred (joined through an event topic or a shared database), uncertain (a \
computed URL or a fuzzy name match). \
Write a short plain-English summary for the pull request: what changed, which services could be \
affected and through what, and what the reviewer should check. Use only the facts in the JSON and do \
not guess at code you cannot see. Say which links are uncertain. Never say a service cannot be \
affected; for services outside the radius, no path was found. \
Plain text only: at most eight short lines, bullets starting with \"- \", no headings, no bold.";

/// The user message: the repository name and the blast radius as JSON.
pub fn prompt(repository: &str, blast: &Blast) -> String {
    let json = serde_json::to_string_pretty(blast).unwrap_or_default();
    format!("Repository: {repository}\n\nBlast radius:\n{json}")
}

/// Claude models that take an effort level and server-side fallbacks.
fn current_claude(model: &str) -> bool {
    [
        "claude-opus-5",
        "claude-sonnet-5",
        "claude-fable-5",
        "claude-mythos-5",
    ]
    .iter()
    .any(|prefix| model.starts_with(prefix))
}

/// One HTTP request, built without sending it.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

pub fn request(config: &LlmConfig, user: &str) -> Request {
    let mut headers = vec![("content-type".to_string(), "application/json".to_string())];
    if config.provider != Provider::Anthropic {
        if let Some(key) = &config.key {
            headers.push(("authorization".into(), format!("Bearer {key}")));
        }
        return Request {
            url: format!("{}/chat/completions", config.url),
            headers,
            body: json!({
                "model": config.model,
                "messages": [
                    {"role": "system", "content": SYSTEM},
                    {"role": "user", "content": user},
                ],
            }),
        };
    }
    if let Some(key) = &config.key {
        headers.push(("x-api-key".into(), key.clone()));
    }
    headers.push(("anthropic-version".into(), "2023-06-01".into()));
    let mut body = json!({
        "model": config.model,
        "max_tokens": 16000,
        "system": SYSTEM,
        "messages": [{"role": "user", "content": user}],
    });
    if current_claude(&config.model) {
        // A summary is a light task; a declined request is retried on the
        // model Anthropic recommends for its category.
        body["output_config"] = json!({"effort": "low"});
        body["fallbacks"] = json!("default");
        headers.push((
            "anthropic-beta".into(),
            "server-side-fallback-2026-07-01".into(),
        ));
    }
    Request {
        url: format!("{}/messages", config.url),
        headers,
        body,
    }
}

/// The text of a successful response body.
pub fn parse(provider: Provider, body: &Value) -> Result<String, ExplainError> {
    let text = match provider {
        Provider::Anthropic => {
            if body["stop_reason"] == "refusal" {
                return Err(ExplainError::Http(
                    "the model declined to explain this change".into(),
                ));
            }
            body["content"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| b["type"] == "text")
                        .filter_map(|b| b["text"].as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default()
        }
        _ => body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(ExplainError::Http(format!(
            "{} returned no text",
            provider.as_str()
        )));
    }
    Ok(text)
}

/// The provider's own message from an error body, when it has one.
fn error_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let message = parsed.as_ref().and_then(|v| {
        v["error"]["message"]
            .as_str()
            .or_else(|| v["error"].as_str())
            .or_else(|| v[0]["error"]["message"].as_str())
            .map(str::to_string)
    });
    let message = message.unwrap_or_else(|| body.trim().to_string());
    message.chars().take(300).collect()
}

/// Sends the request and returns the explanation. The key goes only to the
/// host configured: no redirect is followed.
pub fn explain(
    config: &LlmConfig,
    repository: &str,
    blast: &Blast,
) -> Result<Explanation, ExplainError> {
    let req = request(config, &prompt(repository, blast));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_SECONDS)))
        .max_redirects(0)
        .http_status_as_error(false)
        .build()
        .into();
    let mut call = agent.post(&req.url);
    for (name, value) in &req.headers {
        call = call.header(name, value);
    }
    let fail = |message: String| {
        ExplainError::Http(format!(
            "{} ({}): {message}",
            config.provider.as_str(),
            config.model
        ))
    };
    let mut response = call
        .send(req.body.to_string())
        .map_err(|e| fail(e.to_string()))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|e| fail(format!("reading the response: {e}")))?;
    if !(200..300).contains(&status) {
        return Err(fail(format!("HTTP {status}: {}", error_message(&body))));
    }
    let value: Value =
        serde_json::from_str(&body).map_err(|e| fail(format!("the response is not JSON: {e}")))?;
    Ok(Explanation {
        provider: config.provider,
        model: config.model.clone(),
        text: parse(config.provider, &value).map_err(|e| fail(e.to_string()))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;

    fn env(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn the_provider_comes_from_the_flag_then_the_environment_then_the_keys_found() {
        let c = resolve(None, None, &env(&[("OPENAI_API_KEY", "sk-o")])).unwrap();
        assert_eq!(c.provider, Provider::Openai);
        assert_eq!(c.model, "gpt-6.1-sol");
        assert_eq!(c.url, "https://api.openai.com/v1");
        assert_eq!(c.key.as_deref(), Some("sk-o"));

        let both = env(&[("OPENAI_API_KEY", "sk-o"), ("ANTHROPIC_API_KEY", "sk-a")]);
        assert_eq!(
            resolve(None, None, &both).unwrap().provider,
            Provider::Anthropic
        );
        assert_eq!(
            resolve(Some("openai"), None, &both).unwrap().provider,
            Provider::Openai
        );
        let chosen = env(&[
            ("VERNIER_LLM", "gemini"),
            ("GEMINI_API_KEY", "g"),
            ("ANTHROPIC_API_KEY", "a"),
        ]);
        let c = resolve(None, None, &chosen).unwrap();
        assert_eq!(
            (c.provider, c.model.as_str()),
            (Provider::Gemini, "gemini-3.8-flash")
        );

        let err = resolve(None, None, &env(&[])).unwrap_err();
        assert!(err.to_string().contains("ANTHROPIC_API_KEY"), "{err}");
        let err = resolve(Some("bard"), None, &env(&[])).unwrap_err();
        assert!(
            err.to_string().contains("unknown LLM provider \"bard\""),
            "{err}"
        );
    }

    #[test]
    fn keys_are_required_for_hosted_providers_and_models_for_local_ones() {
        let err = resolve(Some("anthropic"), None, &env(&[])).unwrap_err();
        assert_eq!(
            err.to_string(),
            "--explain with anthropic needs ANTHROPIC_API_KEY in the environment"
        );
        let err = resolve(Some("ollama"), None, &env(&[])).unwrap_err();
        assert!(err.to_string().contains("ollama list"), "{err}");
        let c = resolve(Some("ollama"), Some("llama3.3"), &env(&[])).unwrap();
        assert_eq!(c.url, "http://localhost:11434/v1");
        assert_eq!(c.key, None);

        let err = resolve(Some("openai-compatible"), Some("m"), &env(&[])).unwrap_err();
        assert!(err.to_string().contains("VERNIER_LLM_URL"), "{err}");
        let c = resolve(
            Some("openai-compatible"),
            None,
            &env(&[
                ("VERNIER_LLM_URL", "https://openrouter.ai/api/v1/"),
                ("VERNIER_LLM_MODEL", "x/y"),
                ("VERNIER_LLM_API_KEY", "k"),
            ]),
        )
        .unwrap();
        assert_eq!(c.url, "https://openrouter.ai/api/v1");
        assert_eq!(c.model, "x/y");
        assert_eq!(c.key.as_deref(), Some("k"));
        assert!(!format!("{c:?}").contains("\"k\""), "the key never prints");
    }

    #[test]
    fn anthropic_requests_use_the_messages_api() {
        let c = resolve(
            Some("anthropic"),
            None,
            &env(&[("ANTHROPIC_API_KEY", "sk-a")]),
        )
        .unwrap();
        let r = request(&c, "hello");
        assert_eq!(r.url, "https://api.anthropic.com/v1/messages");
        assert!(r.headers.contains(&("x-api-key".into(), "sk-a".into())));
        assert!(
            r.headers
                .contains(&("anthropic-version".into(), "2023-06-01".into()))
        );
        assert_eq!(r.body["model"], "claude-opus-5-5");
        assert_eq!(r.body["system"], SYSTEM);
        assert_eq!(r.body["messages"][0]["content"], "hello");
        assert_eq!(r.body["output_config"]["effort"], "low");
        assert_eq!(r.body["fallbacks"], "default");

        let older = resolve(
            Some("anthropic"),
            Some("claude-haiku-4-5"),
            &env(&[("ANTHROPIC_API_KEY", "sk-a")]),
        )
        .unwrap();
        let r = request(&older, "hello");
        assert!(r.body.get("output_config").is_none() && r.body.get("fallbacks").is_none());
        assert!(!r.headers.iter().any(|(k, _)| k == "anthropic-beta"));

        let ok = json!({"content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": " - cart changed\n"}], "stop_reason": "end_turn"});
        assert_eq!(parse(Provider::Anthropic, &ok).unwrap(), "- cart changed");
        let refused = json!({"content": [], "stop_reason": "refusal"});
        assert!(parse(Provider::Anthropic, &refused).is_err());
    }

    #[test]
    fn other_providers_use_chat_completions() {
        let c = resolve(Some("gemini"), None, &env(&[("GEMINI_API_KEY", "g")])).unwrap();
        let r = request(&c, "hello");
        assert_eq!(
            r.url,
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert!(
            r.headers
                .contains(&("authorization".into(), "Bearer g".into()))
        );
        assert_eq!(r.body["messages"][0]["role"], "system");
        assert_eq!(r.body["messages"][1]["content"], "hello");

        let local = resolve(Some("ollama"), Some("qwen3"), &env(&[])).unwrap();
        assert!(
            !request(&local, "x")
                .headers
                .iter()
                .any(|(k, _)| k == "authorization")
        );

        let ok =
            json!({"choices": [{"message": {"role": "assistant", "content": "- web calls cart"}}]});
        assert_eq!(parse(Provider::Openai, &ok).unwrap(), "- web calls cart");
        assert!(parse(Provider::Openai, &json!({"choices": []})).is_err());
        assert_eq!(
            error_message(r#"{"error": {"message": "Incorrect API key provided"}}"#),
            "Incorrect API key provided"
        );
    }
}
