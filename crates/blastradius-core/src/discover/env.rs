//! Dotenv files and compose-style `${VAR}` interpolation.
//!
//! Only the dotenv values beside the compose file are used, never the
//! analyst's shell, so two people analysing the same commit get the same
//! answer. Unresolved references are left in place so the evidence shows
//! exactly what could not be resolved.
use std::path::Path;
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::{Captures, Regex};

use crate::fs::read_text;

pub type Env = IndexMap<String, String>;

static LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:export\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)$").unwrap());
static TRAILING_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+#.*$").unwrap());
static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)(?::?-([^}]*))?\}|\$([A-Za-z_][A-Za-z0-9_]*)").unwrap()
});
static UNRESOLVED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$\{?[A-Za-z_]").unwrap());

/// `KEY=value`, optional `export`, quotes, comments.
pub fn parse_dotenv(text: &str) -> Env {
    parse_dotenv_lines(text)
        .into_iter()
        .map(|(key, value, _)| (key, value))
        .collect()
}

/// Every assignment with its 1-based line, in file order.
pub fn parse_dotenv_lines(text: &str) -> Vec<(String, String, u32)> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(caps) = LINE.captures(line) else {
            continue;
        };
        let key = caps[1].to_string();
        let value = caps.get(2).map_or("", |m| m.as_str());
        out.push((
            key,
            unquote(value),
            u32::try_from(i + 1).unwrap_or(u32::MAX),
        ));
    }
    out
}

/// A quoted value ends at its closing quote; anything after it is a comment.
/// An unquoted value ends at ` #`.
fn unquote(value: &str) -> String {
    if let Some(quote) = value.chars().next().filter(|c| *c == '"' || *c == '\'') {
        if let Some(end) = value[1..].find(quote) {
            return value[1..=end].to_string();
        }
    }
    TRAILING_COMMENT.replace(value, "").trim().to_string()
}

/// `${VAR}`, `${VAR:-default}`, `${VAR-default}`, `$VAR`. An empty value
/// counts as unset for `:-`, as in compose.
pub fn interpolate(value: &str, env: &Env) -> String {
    REFERENCE
        .replace_all(value, |caps: &Captures| {
            let key = caps.get(1).or_else(|| caps.get(3)).map(|m| m.as_str());
            let Some(key) = key else {
                return caps[0].to_string();
            };
            match env.get(key) {
                Some(resolved) if !resolved.is_empty() => resolved.clone(),
                _ => caps
                    .get(2)
                    .map_or_else(|| caps[0].to_string(), |m| m.as_str().to_string()),
            }
        })
        .into_owned()
}

pub fn has_unresolved(value: &str) -> bool {
    UNRESOLVED.is_match(value)
}

/// `.env` beside the compose file wins over `.env.example`.
pub fn load_compose_env(compose_dir: &Path) -> Env {
    let mut env = Env::new();
    for file in [".env.example", ".env"] {
        if let Some(text) = read_text(&compose_dir.join(file)) {
            env.extend(parse_dotenv(&text));
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_dotenv() {
        let env = parse_dotenv(
            "# comment\nPLAIN=value\nexport EXPORTED=yes\nQUOTED=\"a # not a comment\"\nSINGLE='x'\nTRAILING=abc # comment\nEMPTY=\nnot a valid line\n",
        );
        let got: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(
            got,
            vec![
                ("PLAIN", "value"),
                ("EXPORTED", "yes"),
                ("QUOTED", "a # not a comment"),
                ("SINGLE", "x"),
                ("TRAILING", "abc"),
                ("EMPTY", ""),
            ]
        );
    }

    #[test]
    fn interpolates_and_leaves_unknown_visible() {
        let mut env = Env::new();
        env.insert("IMAGE".into(), "ghcr.io/x".into());
        env.insert("EMPTY".into(), String::new());
        assert_eq!(
            interpolate("${IMAGE}:${TAG:-latest}", &env),
            "ghcr.io/x:latest"
        );
        assert_eq!(interpolate("${EMPTY:-fallback}", &env), "fallback");
        assert_eq!(interpolate("${TAG-dash}", &env), "dash");
        assert_eq!(interpolate("$IMAGE/svc", &env), "ghcr.io/x/svc");
        assert_eq!(interpolate("${MISSING}", &env), "${MISSING}");
        assert!(has_unresolved("${MISSING}"));
        assert!(!has_unresolved("plain"));
    }

    #[test]
    fn loads_env_beside_compose_file() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../test/fixtures/compose-env-app");
        let env = load_compose_env(&dir);
        assert_eq!(
            env.get("IMAGE_NAME").map(String::as_str),
            Some("ghcr.io/demo")
        );
        assert_eq!(
            env.get("AD_DOCKERFILE").map(String::as_str),
            Some("./src/ad/Dockerfile")
        );
        assert_eq!(env.get("QUOTED").map(String::as_str), Some("quoted value"));
        assert_eq!(env.get("CART_DOCKERFILE"), None);
    }
}
