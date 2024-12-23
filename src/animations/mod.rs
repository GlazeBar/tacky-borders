use crate::user_config::ConfigFormat;
use crate::user_config::CONFIG_FORMAT;
use animation::AnimationParameters;
use animation::AnimationType;
use easing::AnimationEasingImpl;
use parser::parse_animation;
use parser::AnimationParserError;
use parser::IdentifiableAnimationValue;
use rustc_hash::FxHashMap;
use serde::de::Error;
use serde::Deserialize;
use serde::Deserializer;
use serde_jsonc2::Value as JsonValue;
use serde_yml::Value as YamlValue;
use simple_bezier_easing::bezier;
use std::sync::Arc;
use timer::AnimationTimer;

pub mod animation;
mod easing;
mod parser;
pub mod timer;

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AnimationProgress {
    pub fade: f32,
    pub spiral: f32,
    pub angle: f32,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct AnimationFlags {
    pub fade_to_visible: bool,
    pub should_fade: bool,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct Animations {
    #[serde(deserialize_with = "animation", default)]
    pub active: FxHashMap<AnimationType, AnimationParameters>,
    #[serde(deserialize_with = "animation", default)]
    pub inactive: FxHashMap<AnimationType, AnimationParameters>,
    #[serde(default = "default_fps")]
    pub fps: i32,
    #[serde(skip)]
    pub progress: AnimationProgress,
    #[serde(skip)]
    pub flags: AnimationFlags,
    #[serde(skip)]
    pub timer: Option<AnimationTimer>,
}

fn default_fps() -> i32 {
    60
}

fn handle_map<T>(
    map: FxHashMap<AnimationType, T>,
) -> Result<FxHashMap<AnimationType, AnimationParameters>, AnimationParserError>
where
    T: IdentifiableAnimationValue,
{
    map.iter()
        .map(|(animation_type, animation_value)| {
            let (duration, easing) = parse_animation(animation_type, animation_value)?;

            let easing_points = easing.to_points();

            let easing_fn = bezier(
                easing_points[0],
                easing_points[1],
                easing_points[2],
                easing_points[3],
            )
            .map_err(|e| AnimationParserError::Custom(e.to_string()))?;

            Ok((
                animation_type.clone(),
                AnimationParameters {
                    duration,
                    easing_fn: Arc::new(easing_fn),
                },
            ))
        })
        .collect()
}

fn animation<'de, D>(
    deserializer: D,
) -> Result<FxHashMap<AnimationType, AnimationParameters>, D::Error>
where
    D: Deserializer<'de>,
{
    match *CONFIG_FORMAT.read().unwrap() {
        ConfigFormat::Json | ConfigFormat::Jsonc => {
            let map: FxHashMap<AnimationType, JsonValue> =
                FxHashMap::deserialize(deserializer).map_err(D::Error::custom)?;
            handle_map(map).map_err(D::Error::custom)
        }
        ConfigFormat::Yaml => {
            let map: FxHashMap<AnimationType, YamlValue> =
                FxHashMap::deserialize(deserializer).map_err(D::Error::custom)?;
            handle_map(map).map_err(D::Error::custom)
        }
        _ => Err(D::Error::custom("Invalid file type")),
    }
}
