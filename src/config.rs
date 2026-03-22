use std::env;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HelpLanguage {
    Ja,
    #[default]
    En,
}

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub help: HelpConfig,
}

#[derive(Debug, Default, Deserialize)]
pub struct HelpConfig {
    #[serde(default)]
    pub language: HelpLanguage,
}

impl Config {
    pub fn load() -> Self {
        let Some(config_path) = Self::config_path() else {
            return Config::default();
        };

        if let Ok(content) = fs::read_to_string(&config_path) {
            match toml::from_str(&content) {
                Ok(config) => config,
                Err(err) => {
                    eprintln!("Failed to parse config {}: {}", config_path.display(), err);
                    Config::default()
                }
            }
        } else {
            Config::default()
        }
    }

    pub fn config_path() -> Option<PathBuf> {
        if let Ok(config_home) = env::var("XDG_CONFIG_HOME") {
            Some(
                PathBuf::from(config_home)
                    .join("minishelf")
                    .join("config.toml"),
            )
        } else {
            directories::BaseDirs::new().map(|dirs| {
                dirs.home_dir()
                    .join(".config")
                    .join("minishelf")
                    .join("config.toml")
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, HelpLanguage};

    #[test]
    fn help_language_defaults_to_english() {
        let config = Config::default();

        assert_eq!(config.help.language, HelpLanguage::En);
    }

    #[test]
    fn help_language_can_be_loaded_from_config() {
        let config: Config = toml::from_str(
            r#"
[help]
language = "ja"
"#,
        )
        .expect("config should parse");

        assert_eq!(config.help.language, HelpLanguage::Ja);
    }
}
