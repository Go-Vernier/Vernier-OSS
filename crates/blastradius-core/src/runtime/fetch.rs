//! Reading a runtime input: a file, or a URL only when the user gave one.
//! The static path never comes here. Failures are errors with the reason,
//! never silently empty data.
use std::time::Duration;

use super::RuntimeError;

pub const TIMEOUT_SECONDS: u64 = 10;

pub fn is_url(input: &str) -> bool {
    input.starts_with("http://") || input.starts_with("https://")
}

/// A file's contents, or the body of a GET when `input` is a URL.
pub fn read(input: &str) -> Result<String, RuntimeError> {
    if is_url(input) {
        return http_get(input, &[]);
    }
    std::fs::read_to_string(input).map_err(|source| RuntimeError::Io {
        path: input.to_string(),
        source,
    })
}

pub fn http_get(url: &str, headers: &[(&str, &str)]) -> Result<String, RuntimeError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(TIMEOUT_SECONDS)))
        .build()
        .into();
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut response = request
        .call()
        .map_err(|e| RuntimeError::Http(format!("GET {url}: {e}")))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| RuntimeError::Http(format!("GET {url}: reading the body: {e}")))
}

/// Datadog's dependency map for one environment, authenticated from the
/// environment: `DD_API_KEY` and `DD_APP_KEY`. Keys are never flags.
pub fn datadog_live(site: &str, env: &str) -> Result<String, RuntimeError> {
    let api_key = std::env::var("DD_API_KEY").ok().filter(|k| !k.is_empty()).ok_or_else(|| {
        RuntimeError::Config(
            "DD_API_KEY is not set; a live Datadog call needs DD_API_KEY and DD_APP_KEY in the environment".into(),
        )
    })?;
    let application_key = std::env::var("DD_APP_KEY").ok().filter(|k| !k.is_empty()).ok_or_else(|| {
        RuntimeError::Config(
            "DD_APP_KEY is not set; a live Datadog call needs DD_API_KEY and DD_APP_KEY in the environment".into(),
        )
    })?;
    let url = super::datadog::url(site, env);
    http_get(
        &url,
        &[
            ("DD-API-KEY", &api_key),
            ("DD-APPLICATION-KEY", &application_key),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_recognised_and_live_datadog_needs_keys() {
        assert!(is_url("http://collector:8889/metrics") && is_url("https://x/y"));
        assert!(!is_url("traces.prom") && !is_url("/tmp/x.json") && !is_url("httpd.conf"));
        // The test binary's environment: make sure the keys are absent, then expect a Config error naming them.
        // SAFETY: this is the only test that touches these variables, and nothing else in the
        // process reads them while this test runs.
        unsafe {
            std::env::remove_var("DD_API_KEY");
            std::env::remove_var("DD_APP_KEY");
        }
        let err = datadog_live("datadoghq.com", "prod").unwrap_err();
        assert!(matches!(err, RuntimeError::Config(_)), "{err}");
        assert!(err.to_string().contains("DD_API_KEY"), "{err}");
    }
}
