use std::env;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub layout: LayoutConfig,
}

#[derive(Debug, Deserialize)]
pub struct LayoutConfig {
    pub tree_ratio_normal: u16,
    pub tree_ratio_preview_focused: u16,
}

impl Default for LayoutConfig {
    fn default() -> Self {
        Self {
            tree_ratio_normal: 50,
            tree_ratio_preview_focused: 10,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            layout: LayoutConfig::default(),
        }
    }
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
            Some(PathBuf::from(config_home).join("minishelf").join("config.toml"))
        } else {
            directories::BaseDirs::new().map(|dirs| {
                dirs.home_dir().join(".config").join("minishelf").join("config.toml")
            })
        }
    }
}
