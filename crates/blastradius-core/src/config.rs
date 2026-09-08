//! `blast-radius.config.json` at the repository root. Read only when a stage
//! needs it; absent means defaults; invalid means an error, because a file
//! the user wrote must not be silently skipped. Unknown keys are ignored so
//! later stages can add their own.
use std::path::Path;

use indexmap::IndexMap;
use serde::Deserialize;
use thiserror::Error;

use crate::fs::read_text;

pub const FILE_NAME: &str = "blast-radius.config.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

/// How runtime service names map onto discovered services when the
/// automatic tiers get it wrong, and which runtime names to leave out.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default)]
    pub map: IndexMap<String, String>,
    #[serde(default)]
    pub ignore: Vec<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("{path}: {message}")]
    Invalid { path: String, message: String },
}

pub fn load(root: &Path) -> Result<Config, ConfigError> {
    let path = root.join(FILE_NAME);
    let Some(text) = read_text(&path) else {
        return Ok(Config::default());
    };
    serde_json::from_str(&text).map_err(|e| ConfigError::Invalid {
        path: path.display().to_string(),
        message: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../test/fixtures")
            .join(name)
    }

    #[test]
    fn absent_file_is_the_default_and_present_file_is_read() {
        assert_eq!(load(&fixture("compose-app")).unwrap(), Config::default());
        let cfg = load(&fixture("runtime-app")).unwrap();
        assert_eq!(
            cfg.runtime.map.get("pay").map(String::as_str),
            Some("payment")
        );
        assert_eq!(cfg.runtime.ignore, vec!["load-generator"]);
    }

    #[test]
    fn invalid_json_is_an_error_naming_the_file() {
        let root =
            std::env::temp_dir().join(format!("blast-radius-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(FILE_NAME), "{ not json").unwrap();
        let err = load(&root).unwrap_err();
        assert!(err.to_string().contains(FILE_NAME), "{err}");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
