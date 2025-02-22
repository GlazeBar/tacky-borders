use crate::animation::AnimationsConfig;
use crate::border_manager::reload_borders;
use crate::colors::GlobalColor;
use crate::core::app_state::APP_STATE;
use crate::core::dimension::deserialize_dimension;
use crate::core::dimension::deserialize_optional_dimension;
use crate::core::keybindings::Keybindings;
use crate::create_keybindings;
use crate::error::LogIfErr;
use crate::keyboard_hook::KEYBOARD_HOOK;
use crate::windows_api::WindowsApi;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result as AnyResult;
use schema_jsonrs::JsonSchema;
use serde::de;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use std::fs::exists;
use std::fs::read_to_string;
use std::fs::write;
use std::fs::DirBuilder;
use std::path::Path;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::RwLock;
use windows::Win32::Graphics::Dwm::DWMWCP_DEFAULT;
use windows::Win32::Graphics::Dwm::DWMWCP_DONOTROUND;
use windows::Win32::Graphics::Dwm::DWMWCP_ROUND;
use windows::Win32::Graphics::Dwm::DWMWCP_ROUNDSMALL;

/// Default configuration content stored as a YAML string.
const DEFAULT_CONFIG: &str = include_str!("../resources/config.yaml");

pub static CONFIG_FORMAT: LazyLock<RwLock<ConfigFormat>> =
    LazyLock::new(|| RwLock::new(ConfigFormat::default()));

/// Represents the supported configuration file formats.
#[derive(Debug, Clone, Default)]
pub enum ConfigFormat {
    #[cfg(feature = "json")]
    /// JSON configuration file.
    Json,
    #[cfg(feature = "json")]
    /// JSON with comments (JSONC) configuration file.
    Jsonc,
    #[cfg(feature = "yml")]
    /// YAML configuration file.
    Yaml,
    /// Placeholder for cases where no configuration type is detected.
    #[default]
    None,
}

/// Defines options for border radius customization.
#[derive(Debug, PartialEq, Clone, Default, JsonSchema)]
pub enum BorderStyle {
    /// Fully rounded borders.
    Round,
    /// Square borders with no rounding.
    Square,
    /// Small rounding for subtle border effects.
    SmallRound,
    /// Automatically determine the border radius based on context.
    #[default]
    Auto,
    /// Custom border radius, specified in pixels.
    Radius(f32),
}

impl<'de> Deserialize<'de> for BorderStyle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.eq_ignore_ascii_case("ROUND") {
            Ok(BorderStyle::Round)
        } else if s.eq_ignore_ascii_case("SQUARE") {
            Ok(BorderStyle::Square)
        } else if s.eq_ignore_ascii_case("SMALLROUND") {
            Ok(BorderStyle::SmallRound)
        } else if s.eq_ignore_ascii_case("AUTO") {
            Ok(BorderStyle::Auto)
        } else if s.to_ascii_uppercase().starts_with("RADIUS(") && s.ends_with(")") {
            let inner = &s[7..s.len() - 1];
            inner
                .parse::<f32>()
                .map(BorderStyle::Radius)
                .map_err(|_| de::Error::custom("Invalid Radius value"))
        } else {
            Err(de::Error::custom("Invalid border style"))
        }
    }
}

impl BorderStyle {
    pub fn to_radius(&self, border_width: i32, dpi: f32, tracking_window: isize) -> f32 {
        let base_radius = (border_width as f32) / 2.0;
        let scale_factor = dpi / 96.0;

        match self {
            BorderStyle::Radius(-1.0) | BorderStyle::Auto => {
                match WindowsApi::get_window_corner_preference(tracking_window) {
                    DWMWCP_DEFAULT | DWMWCP_ROUND => 8.0 * scale_factor + base_radius,
                    DWMWCP_ROUNDSMALL => 4.0 * scale_factor + base_radius,
                    DWMWCP_DONOTROUND => 0.0,
                    _ => base_radius, // fallback default
                }
            }
            BorderStyle::Round => 8.0 * scale_factor + base_radius,
            BorderStyle::SmallRound => 4.0 * scale_factor + base_radius,
            BorderStyle::Square => 0.0,
            BorderStyle::Radius(radius) => radius * scale_factor,
        }
    }
}

/// Specifies the type of match used for window identification.
#[derive(Debug, Deserialize, Serialize, PartialEq, Clone, JsonSchema)]
pub enum MatchKind {
    /// Match based on the window title.
    Title,
    /// Match based on the class name of the window.
    Class,
    /// Match based on the process name or executable associated with the window.
    Process,
}

/// Defines the strategy for matching a value against a criterion.
#[derive(Debug, Deserialize, PartialEq, Clone, JsonSchema)]
pub enum MatchStrategy {
    /// Match values that are exactly equal.
    Equals,
    /// Match values using a regular expression.
    Regex,
    /// Match values that contain the specified substring.
    Contains,
}

/// Represents criteria used to match windows for applying specific configurations.
#[derive(Debug, Deserialize, Clone, Default, PartialEq, JsonSchema)]
pub struct WindowMatchConfig {
    /// Type of match (e.g., title, class, or process).
    #[serde(rename = "kind")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_kind: Option<MatchKind>,
    /// The value to match against (e.g., window title or class name).
    #[serde(rename = "value")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_value: Option<String>,
    /// Strategy for matching, such as exact match or regex.
    #[serde(rename = "strategy")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_strategy: Option<MatchStrategy>,
    /// Color for the border when the window is active.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_color: Option<GlobalColor>,
    /// Color for the border when the window is inactive.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inactive_color: Option<GlobalColor>,
    /// Animation settings for the window borders.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animations: Option<AnimationsConfig>,
    /// Radius of the border corners.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border_style: Option<BorderStyle>,
    /// Width of the border in pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_optional_dimension", default)]
    pub border_width: Option<i32>,
    /// Offset of the border relative to the window.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(deserialize_with = "deserialize_optional_dimension", default)]
    pub border_offset: Option<i32>,
    /// Whether borders are enabled for this match.
    #[serde(rename = "enabled")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Delay (in milliseconds) before applying the border after initialization.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initialize_delay: Option<u64>,
    /// Delay (in milliseconds) before applying the border after unminimizing.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(alias = "restore_delay")]
    pub unminimize_delay: Option<u64>,
}

/// Represents a rule for a specific window, including matching criteria and associated actions.
#[derive(Debug, Deserialize, Clone, Default, PartialEq, JsonSchema)]
pub struct WindowRuleConfig {
    /// The matching details and settings for a specific type of window.
    #[serde(rename = "match")]
    pub match_window: WindowMatchConfig,
}

fn serde_default_global() -> GlobalRuleConfig {
    GlobalRuleConfig {
        border_width: serde_default_i32::<2>(),
        border_offset: serde_default_i32::<-1>(),
        ..Default::default()
    }
}

/// Contains global configuration settings applied across all windows.
#[derive(Debug, Deserialize, Clone, Default, PartialEq, JsonSchema)]
#[serde(default)]
pub struct GlobalRuleConfig {
    /// Default width of the window borders.
    #[serde(
        deserialize_with = "deserialize_dimension",
        default = "serde_default_i32::<2>"
    )]
    pub border_width: i32,
    /// Default offset for the window borders.
    #[serde(
        deserialize_with = "deserialize_dimension",
        default = "serde_default_i32::<-1>"
    )]
    pub border_offset: i32,
    /// Default border radius settings.
    #[serde(default)]
    pub border_style: BorderStyle,
    /// Default color for active window borders.
    #[serde(default)]
    pub active_color: GlobalColor,
    /// Default color for inactive window borders.
    #[serde(default)]
    pub inactive_color: GlobalColor,
    /// Animation settings for borders.
    #[serde(default)]
    pub animations: AnimationsConfig,
    /// Delay (in milliseconds) before applying borders after initialization.
    #[serde(alias = "init_delay", default = "serde_default_u64::<250>")]
    pub initialize_delay: u64,
    /// Delay (in milliseconds) before applying borders after unminimizing.
    #[serde(alias = "restore_delay", default = "serde_default_u64::<200>")]
    pub unminimize_delay: u64,
}

/// Stores the complete configuration including global rules, window rules, and keybindings.
#[derive(Debug, Deserialize, Clone, Default, PartialEq, JsonSchema)]
#[serde(default)]
pub struct UserConfig {
    /// Global settings applied across all windows.
    #[serde(rename = "global", default = "serde_default_global")]
    pub global_rule: GlobalRuleConfig,
    /// Specific rules for individual windows.
    #[serde(default)]
    pub window_rules: Vec<WindowRuleConfig>,
    /// Application keybindings.
    #[serde(default)]
    pub keybindings: Keybindings,
    /// Enables monitoring for changes in the configuration file.
    #[serde(default)]
    pub monitor_config_changes: bool,
    /// Enable custom predefined theme
    pub theme: Option<String>,
}

/// Methods for managing the configuration, including loading, saving, and reloading.
impl UserConfig {
    /// Attempts to create a new configuration instance by reading from the config file.
    pub fn create() -> AnyResult<Self> {
        let config_dir = UserConfig::get_config_dir().unwrap_or_default();
        let config_file = UserConfig::detect_config_file(&config_dir)
            .unwrap_or(Self::create_default_config(&config_dir).unwrap_or_default());
        let config_format = UserConfig::detect_config_format(&config_dir).unwrap_or_default();
        let contents = read_to_string(&config_file)
            .with_context(|| format!("failed to read config file: {}", config_file.display()))?;

        *CONFIG_FORMAT.write().unwrap() = config_format.clone();
        let config = Self::deserialize(contents)?;

        Ok(config)
    }

    /// Deserializes configuration content into a `Config` instance based on the file format.
    fn deserialize(contents: String) -> AnyResult<Self> {
        let config_format = &*CONFIG_FORMAT
            .read()
            .map_err(|_| anyhow!("config format lock poisoned"))?;

        #[cfg(feature = "json")]
        if matches!(config_format, ConfigFormat::Json | ConfigFormat::Jsonc) {
            return serde_jsonc2::from_str(&contents).with_context(|| "failed to deserialize JSON");
        }

        #[cfg(feature = "yml")]
        if matches!(config_format, ConfigFormat::Yaml) {
            return serde_yml::from_str(&contents).with_context(|| "failed to deserialize YAML");
        }

        Err(anyhow!("unsupported configuration format"))
    }

    /// Detects the configuration file in the given directory or creates a default config file if none exists.
    pub fn detect_config_file(config_dir: &Path) -> AnyResult<PathBuf> {
        let candidates = [
            "json",
            #[cfg(feature = "json")]
            "json",
            #[cfg(feature = "json")]
            "jsonc",
            #[cfg(feature = "yml")]
            "yaml",
            #[cfg(feature = "yml")]
            "yml",
        ];

        for ext in candidates {
            let file_path = config_dir.join("config").with_extension(ext);
            if exists(file_path.clone())? {
                return Ok(file_path);
            }
        }

        Err(anyhow!("config file not found"))
    }

    /// Creates a default configuration file in the specified directory.
    pub fn create_default_config(config_dir: &Path) -> AnyResult<PathBuf> {
        let path = config_dir.join("config.yaml");
        write(path.clone(), DEFAULT_CONFIG.as_bytes())
            .with_context(|| format!("failed to write default config to {}", path.display()))?;

        Ok(path.clone())
    }

    pub fn detect_config_format(config_dir: &Path) -> AnyResult<ConfigFormat> {
        let candidates = [
            #[cfg(feature = "json")]
            ("json", ConfigFormat::Json),
            #[cfg(feature = "json")]
            ("jsonc", ConfigFormat::Jsonc),
            #[cfg(feature = "yml")]
            ("yaml", ConfigFormat::Yaml),
            #[cfg(feature = "yml")]
            ("yml", ConfigFormat::Yaml),
        ];

        for (ext, config_type) in candidates {
            let file_path = config_dir.join("config").with_extension(ext);
            if exists(file_path.clone())? {
                return Ok(config_type);
            }
        }

        #[cfg(all(feature = "yml", not(feature = "json")))]
        {
            Ok(ConfigFormat::Yaml)
        }

        #[cfg(all(feature = "json", not(feature = "yml")))]
        {
            Ok(ConfigFormat::Json)
        }

        #[cfg(all(feature = "json", feature = "yml"))]
        {
            Ok(ConfigFormat::Json) // Priority is YAML
        }

        #[cfg(not(any(feature = "json", feature = "yml")))]
        {
            Err(anyhow::anyhow!("No supported config format found"))
        }
    }

    /// Retrieves the configuration directory, creating it if necessary.
    pub fn get_config_dir() -> AnyResult<PathBuf> {
        let home_dir = WindowsApi::home_dir()?;
        let config_dir = home_dir.join(".config").join("tacky-borders");
        let fallback_dir = home_dir.join(".tacky-borders");

        if exists(config_dir.clone())
            .with_context(|| format!("could not find directory: {}", config_dir.display()))?
        {
            return Ok(config_dir);
        } else if exists(fallback_dir.clone())
            .with_context(|| format!("could not find directory: {}", fallback_dir.display()))?
        {
            return Ok(fallback_dir);
        }

        DirBuilder::new()
            .recursive(true)
            .create(&config_dir)
            .map_err(|_| anyhow!("could not create config directory"))?;

        Ok(config_dir)
    }

    /// Update the configuration by reinitializing it from the configuration file.
    ///
    /// This method replaces the current configuration with a newly loaded one.
    /// If loading fails, it falls back to the default configuration and logs an error.
    pub fn update() {
        let new_config = match Self::create() {
            Ok(config) => {
                let mut config_watcher = APP_STATE.config_watcher.write().unwrap();

                if config.monitor_config_changes && !config_watcher.is_running() {
                    config_watcher.start().log_if_err();
                } else if !config.monitor_config_changes && config_watcher.is_running() {
                    config_watcher.stop().log_if_err();
                }
                config
            }
            Err(e) => {
                error!("could not reload config: {e}");
                UserConfig::default()
            }
        };

        *APP_STATE.config.write().unwrap() = new_config;
    }

    /// Reloads the application configuration and restarts the borders.
    ///
    /// This method calls the `update()` function to reload the configuration and refresh the
    /// application state. After updating the configuration, it restarts the borders and updates
    /// the keyboard bindings if they are available.
    ///
    /// # Side Effects
    /// - The configuration is reloaded from the file and written to the shared configuration store.
    /// - The borders are reloaded, which may involve reinitializing UI components.
    /// - If a keyboard hook is available, the keybindings are refreshed and applied.
    pub fn reload() -> bool {
        debug!("reloading application configuration and restarting borders.");
        let old_config = (*APP_STATE.config.read().unwrap()).clone();
        Self::update();
        let new_config = APP_STATE.config.read().unwrap();

        if old_config != *new_config {
            reload_borders();
            if let Some(hook) = KEYBOARD_HOOK.get() {
                if let Ok(bindings) = create_keybindings() {
                    hook.update(&bindings);
                }
            }
            return true;
        }
        false
    }

    /// Opens the configuration file in the default editor.
    ///
    /// This method determines the configuration file's path based on the current config type
    /// (e.g., JSON, YAML, JSONC) and attempts to open it using the default file association on the system.
    pub fn open() {
        match Self::get_config_dir() {
            Ok(dir) => {
                let config_file_res = Self::detect_config_file(dir.as_path());
                match config_file_res {
                    Ok(config_file) => {
                        win_open::that(config_file).log_if_err();
                    }
                    Err(e) => {
                        error!("{e}");
                    }
                }
            }
            Err(err) => error!("{err}"),
        }
    }
}

// Helpers
fn serde_default_u64<const V: u64>() -> u64 {
    V
}

fn serde_default_i32<const V: i32>() -> i32 {
    V
}
