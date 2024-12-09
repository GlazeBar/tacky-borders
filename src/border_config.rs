use crate::animations::Animations;
use crate::utils::home_dir;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result as AnyResult;
use serde::Deserialize;
use serde::Serialize;
use std::fs::exists;
use std::fs::read_to_string;
use std::fs::write;
use std::fs::DirBuilder;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::Mutex;
use win_color::GlobalColor;

pub static CONFIG: LazyLock<Mutex<Config>> = LazyLock::new(|| {
    Mutex::new(match Config::new() {
        Ok(config) => config,
        Err(err) => {
            error!("Error: {}", err);
            Config::default()
        }
    })
});

pub static CONFIG_TYPE: LazyLock<Mutex<&str>> = LazyLock::new(|| Mutex::new(""));

const DEFAULT_CONFIG: &str = include_str!("../resources/config.yaml");

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub enum BorderRadiusOption {
    Round,
    Square,
    SmallRound,
    Auto,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
#[serde(untagged)]
pub enum BorderRadius {
    String(BorderRadiusOption),
    Float(f32),
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Clone)]
pub enum MatchKind {
    Title,
    Class,
    Process,
}

#[derive(Debug, Deserialize, PartialEq, Clone)]
pub enum MatchStrategy {
    Equals,
    Regex,
    Contains,
}

impl Default for BorderRadius {
    fn default() -> Self {
        Self::Float(0.0)
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct MatchDetails {
    #[serde(rename = "kind")]
    pub match_kind: Option<MatchKind>,
    #[serde(rename = "value")]
    pub match_value: Option<String>,
    #[serde(rename = "strategy")]
    pub match_strategy: Option<MatchStrategy>,
    pub active_color: Option<GlobalColor>,
    pub inactive_color: Option<GlobalColor>,
    pub animations: Option<Animations>,
    pub border_radius: Option<BorderRadius>,
    pub border_width: Option<f32>,
    pub border_offset: Option<i32>,
    #[serde(rename = "enabled")]
    pub border_enabled: Option<bool>,
    pub initialize_delay: Option<u64>,
    pub unminimize_delay: Option<u64>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct WindowRule {
    #[serde(rename = "match")]
    pub rule_match: MatchDetails,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct GlobalRule {
    pub border_width: f32,
    pub border_offset: i32,
    pub border_radius: BorderRadius,
    pub active_color: GlobalColor,
    pub inactive_color: GlobalColor,
    pub animations: Option<Animations>,
    pub initialize_delay: Option<u64>,
    pub unminimize_delay: Option<u64>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Config {
    #[serde(rename = "global")]
    pub global_rule: GlobalRule,
    pub window_rules: Vec<WindowRule>,
}

impl Config {
    fn new() -> AnyResult<Self> {
        let config_dir = Self::get_config_dir()?;
        let config_path = config_dir.join("config");

        let json_path = config_path.with_extension("json");
        let yaml_path = config_path.with_extension("yaml");
        let toml_path = config_path.with_extension("toml");

        let config_file = if exists(yaml_path.clone())? {
            *CONFIG_TYPE.lock().unwrap() = "yaml";
            yaml_path
        } else if exists(json_path.clone())? {
            *CONFIG_TYPE.lock().unwrap() = "json";
            json_path.clone()
        } else if exists(toml_path.clone())? {
            *CONFIG_TYPE.lock().unwrap() = "toml";
            toml_path.clone()
        } else {
            *CONFIG_TYPE.lock().unwrap() = "yaml";
            Self::create_default_config(&yaml_path.clone())?;
            info!(r"generating default config in {}", yaml_path.display());
            yaml_path.clone()
        };

        let contents = read_to_string(&config_file)
            .with_context(|| format!("Failed to read config file: {}", config_file.display()))?;

        let config: Config = match config_file
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
        {
            "json" => serde_json::from_str(&contents)
                .with_context(|| "Failed to deserialize config.json")?,
            "yaml" => serde_yaml_ng::from_str(&contents)
                .with_context(|| "Failed to deserialize config.yaml")?,
            "toml" => {
                toml::from_str(&contents).with_context(|| "Failed to deserialize config.toml")?
            }
            _ => return Err(anyhow!("Unsupported config file format")),
        };

        Ok(config)
    }

    fn create_default_config(path: &PathBuf) -> AnyResult<()> {
        write(path, DEFAULT_CONFIG.as_bytes())
            .with_context(|| format!("Failed to write default config to {}", path.display()))?;
        Ok(())
    }

    pub fn get_config_dir() -> AnyResult<PathBuf> {
        let home_dir = home_dir()?;

        let config_dir = home_dir.join(".config").join("tacky-borders");
        let fallback_dir = home_dir.join(".tacky-borders");

        if exists(config_dir.clone())
            .with_context(|| format!("Could not find {}", config_dir.display()))?
        {
            return Ok(config_dir);
        } else if exists(fallback_dir.clone())
            .with_context(|| format!("Could not find {}", fallback_dir.display()))?
        {
            return Ok(fallback_dir);
        }

        DirBuilder::new()
            .recursive(true)
            .create(&config_dir)
            .map_err(|_| anyhow!("Could not create config directory"))?;

        Ok(config_dir)
    }

    pub fn reload() {
        match Self::new() {
            Ok(config) => *CONFIG.lock().unwrap() = config,
            Err(err) => {
                error!("Error reloading config: {}", err);
                *CONFIG.lock().unwrap() = Config::default();
            }
        }
    }
}
