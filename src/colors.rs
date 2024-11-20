use regex::Regex;
use serde::Deserialize;
use std::f32::consts::PI;
use std::sync::LazyLock;
use windows::Win32::Foundation::BOOL;
use windows::Win32::Foundation::FALSE;
use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP;
use windows::Win32::Graphics::Direct2D::Common::D2D_POINT_2F;
use windows::Win32::Graphics::Direct2D::ID2D1Brush;
use windows::Win32::Graphics::Direct2D::ID2D1HwndRenderTarget;
use windows::Win32::Graphics::Direct2D::D2D1_BRUSH_PROPERTIES;
use windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE_CLAMP;
use windows::Win32::Graphics::Direct2D::D2D1_GAMMA_2_2;
use windows::Win32::Graphics::Direct2D::D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES;
use windows::Win32::Graphics::Dwm::DwmGetColorizationColor;

use crate::utils::strip_string;
use crate::windowsapi::WindowsApi;

// Constants
const COLOR_PATTERN: &str = r"(?i)#[0-9A-F]{3,8}|rgba?\([0-9]{1,3},\s*[0-9]{1,3},\s*[0-9]{1,3}(?:,\s*[0-9]*(?:\.[0-9]+)?)?\)|accent|transparent";
static COLOR_REGEX: LazyLock<Regex> = LazyLock::new(|| Regex::new(COLOR_PATTERN).unwrap());

// Enums
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum GradientDirection {
    String(String),
    Coordinates(GradientDirectionCoordinates),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ColorConfig {
    String(String),
    Mapping(GradientConfig),
}

#[derive(Debug, Clone)]
pub enum Color {
    Solid(Solid),
    Gradient(Gradient),
}
// Structs
#[derive(Debug, Clone, Deserialize)]
pub struct GradientDirectionCoordinates {
    pub start: [f32; 2],
    pub end: [f32; 2],
}

#[derive(Debug, Clone, Deserialize)]
pub struct GradientConfig {
    pub colors: Vec<String>,
    pub direction: GradientDirection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    pub color: D2D1_COLOR_F,
}

#[derive(Debug, Clone)]
pub struct Gradient {
    pub direction: GradientDirectionCoordinates,
    pub gradient_stops: Vec<D2D1_GRADIENT_STOP>,
}

// Traits
trait ToColor {
    fn to_d2d1_color(self, is_active: Option<bool>) -> D2D1_COLOR_F;
}

// Implementations
impl ToColor for String {
    fn to_d2d1_color(self, is_active_color: Option<bool>) -> D2D1_COLOR_F {
        if self == "accent" {
            let mut pcr_colorization: u32 = 0;
            let mut pf_opaqueblend: BOOL = FALSE;
            let result =
                unsafe { DwmGetColorizationColor(&mut pcr_colorization, &mut pf_opaqueblend) };

            if result.is_err() {
                error!("could not retrieve Windows accent color!");
            }

            let r = ((pcr_colorization & 0x00FF0000) >> 16) as f32 / 255.0;
            let g = ((pcr_colorization & 0x0000FF00) >> 8) as f32 / 255.0;
            let b = (pcr_colorization & 0x000000FF) as f32 / 255.0;
            let avg = (r + g + b) / 3.0;

            return match is_active_color {
                Some(true) => D2D1_COLOR_F { r, g, b, a: 1.0 },
                _ => D2D1_COLOR_F {
                    r: avg / 1.5 + r / 10.0,
                    g: avg / 1.5 + g / 10.0,
                    b: avg / 1.5 + b / 10.0,
                    a: 1.0,
                },
            };
        } else if self.starts_with("#") {
            if self.len() != 7 && self.len() != 9 && self.len() != 4 && self.len() != 5 {
                error!("{}", format!("Invalid hex color format: {}", self).as_str());
                return D2D1_COLOR_F::default();
            }

            let hex = match self.len() {
                4 | 5 => format!(
                    "#{}{}{}{}",
                    self.get(1..2).unwrap_or("").repeat(2),
                    self.get(2..3).unwrap_or("").repeat(2),
                    self.get(3..4).unwrap_or("").repeat(2),
                    self.get(4..5).unwrap_or("").repeat(2)
                ),
                _ => self.to_string(),
            };

            // Parse RGB and Alpha
            let (r, g, b, a) = (
                u8::from_str_radix(&hex[1..3], 16).unwrap_or(0) as f32 / 255.0,
                u8::from_str_radix(&hex[3..5], 16).unwrap_or(0) as f32 / 255.0,
                u8::from_str_radix(&hex[5..7], 16).unwrap_or(0) as f32 / 255.0,
                if hex.len() == 9 {
                    u8::from_str_radix(&hex[7..9], 16).unwrap_or(0) as f32 / 255.0
                } else {
                    1.0
                },
            );

            return D2D1_COLOR_F { r, g, b, a };
        } else if self.starts_with("rgb(") || self.starts_with("rgba(") {
            let rgba = strip_string(self, &["rgb(", "rgba("], ')');
            let components: Vec<&str> = rgba.split(',').map(|s| s.trim()).collect();
            if components.len() == 3 || components.len() == 4 {
                let r: f32 = components[0].parse::<u32>().unwrap_or(0) as f32 / 255.0;
                let g: f32 = components[1].parse::<u32>().unwrap_or(0) as f32 / 255.0;
                let b: f32 = components[2].parse::<u32>().unwrap_or(0) as f32 / 255.0;
                let a = components
                    .get(3)
                    .and_then(|s| s.parse::<f32>().ok())
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0);

                return D2D1_COLOR_F { r, g, b, a };
            }

            return D2D1_COLOR_F::default();
        }

        D2D1_COLOR_F::default()
    }
}

impl From<&String> for GradientDirectionCoordinates {
    fn from(value: &String) -> Self {
        if let Some(degree) = value
            .strip_suffix("deg")
            .and_then(|d| d.trim().parse::<f32>().ok())
        {
            let rad = (degree - 90.0) * PI / 180.0;
            let (cos, sin) = (rad.cos(), rad.sin());

            // Adjusting calculations based on the origin being (0.5, 0.5)
            return GradientDirectionCoordinates {
                start: [0.5 - 0.5 * cos, 0.5 - 0.5 * sin],
                end: [0.5 + 0.5 * cos, 0.5 + 0.5 * sin],
            };
        }

        match value.as_str() {
            "to right" => GradientDirectionCoordinates {
                start: [0.0, 0.5],
                end: [1.0, 0.5],
            },
            "to left" => GradientDirectionCoordinates {
                start: [1.0, 0.5],
                end: [0.0, 0.5],
            },
            "to top" => GradientDirectionCoordinates {
                start: [0.5, 1.0],
                end: [0.5, 0.0],
            },
            "to bottom" => GradientDirectionCoordinates {
                start: [0.5, 0.0],
                end: [0.5, 1.0],
            },
            "to top right" => GradientDirectionCoordinates {
                start: [0.0, 1.0],
                end: [1.0, 0.0],
            },
            "to top left" => GradientDirectionCoordinates {
                start: [1.0, 1.0],
                end: [0.0, 0.0],
            },
            "to bottom right" => GradientDirectionCoordinates {
                start: [0.0, 0.0],
                end: [1.0, 1.0],
            },
            "to bottom left" => GradientDirectionCoordinates {
                start: [1.0, 0.0],
                end: [0.0, 1.0],
            },
            _ => GradientDirectionCoordinates {
                start: [0.5, 1.0],
                end: [0.5, 0.0],
            },
        }
    }
}

impl Color {
    fn from_string(color: String, is_active: Option<bool>) -> Self {
        if color.starts_with("gradient(") && color.ends_with(")") {
            return Self::from_string(strip_string(color, &["gradient("], ')'), is_active);
        }

        let color_re = &COLOR_REGEX;

        // Collect valid colors using regex
        let color_matches: Vec<&str> = color_re
            .captures_iter(&color)
            .filter_map(|cap| cap.get(0).map(|m| m.as_str()))
            .collect();

        if color_matches.len() == 1 {
            return Self::Solid(Solid {
                color: color_matches[0].to_string().to_d2d1_color(is_active),
            });
        }

        let remaining_input = color[color.rfind(color_matches.last().unwrap()).unwrap()
            + color_matches.last().unwrap().len()..]
            .trim_start();

        let remaining_input_arr: Vec<&str> = remaining_input
            .split(',')
            .filter_map(|s| {
                let trimmed = s.trim();
                (!trimmed.is_empty()).then_some(trimmed)
            })
            .collect();

        let direction = remaining_input_arr
            .iter()
            .find(|&&input| is_valid_direction(input))
            .map(|&s| s.to_string())
            .unwrap_or_else(|| "to_right".to_string());
        let colors: Vec<D2D1_COLOR_F> = color_matches
            .iter()
            .map(|&color| color.to_string().to_d2d1_color(is_active))
            .collect();

        let num_colors = colors.len();
        let step = 1.0 / (num_colors - 1) as f32;

        let gradient_stops = colors
            .into_iter()
            .enumerate()
            .map(|(i, color)| D2D1_GRADIENT_STOP {
                position: i as f32 * step,
                color,
            })
            .collect();

        let direction = GradientDirectionCoordinates::from(&direction);

        Self::Gradient(Gradient {
            gradient_stops,
            direction,
        })
    }

    fn from_mapping(color: GradientConfig, is_active: Option<bool>) -> Self {
        match color.colors.len() {
            0 => Color::Solid(Solid {
                color: D2D1_COLOR_F::default(),
            }),
            1 => Color::Solid(Solid {
                color: color.colors[0].clone().to_d2d1_color(is_active),
            }),
            _ => {
                let num_colors = color.colors.len();
                let step = 1.0 / (num_colors - 1) as f32;
                let gradient_stops: Vec<D2D1_GRADIENT_STOP> = color
                    .colors
                    .iter()
                    .enumerate()
                    .map(|(i, hex)| D2D1_GRADIENT_STOP {
                        position: i as f32 * step,
                        color: hex.to_string().to_d2d1_color(is_active),
                    })
                    .collect();

                let direction = match color.direction {
                    GradientDirection::String(direction) => {
                        GradientDirectionCoordinates::from(&direction)
                    }
                    GradientDirection::Coordinates(direction) => direction,
                };

                Color::Gradient(Gradient {
                    gradient_stops,
                    direction,
                })
            }
        }
    }

    pub fn from(color_definition: &ColorConfig, is_active: Option<bool>) -> Self {
        match color_definition {
            ColorConfig::String(s) => Self::from_string(s.clone(), is_active),
            ColorConfig::Mapping(gradient_def) => {
                Self::from_mapping(gradient_def.clone(), is_active)
            }
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::Solid(Solid {
            color: D2D1_COLOR_F::default(),
        })
    }
}

impl Color {
    pub fn create_brush(
        &mut self,
        render_target: &ID2D1HwndRenderTarget, //&ID2D1HwndRenderTarget,
        window_rect: &RECT,
        brush_properties: &D2D1_BRUSH_PROPERTIES,
    ) -> Option<ID2D1Brush> {
        match self {
            Color::Solid(solid) => unsafe {
                let Ok(brush) =
                    render_target.CreateSolidColorBrush(&solid.color, Some(brush_properties))
                else {
                    return None;
                };
                Some(brush.into())
            },
            Color::Gradient(gradient) => unsafe {
                let width = WindowsApi::get_rect_width(*window_rect) as f32;
                let height = WindowsApi::get_rect_height(*window_rect) as f32;

                let gradient_properties = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                    startPoint: D2D_POINT_2F {
                        x: gradient.direction.start[0] * width,
                        y: gradient.direction.start[1] * height,
                    },
                    endPoint: D2D_POINT_2F {
                        x: gradient.direction.end[0] * width,
                        y: gradient.direction.end[1] * height,
                    },
                };

                let Ok(gradient_stop_collection) = render_target.CreateGradientStopCollection(
                    &gradient.gradient_stops,
                    D2D1_GAMMA_2_2,
                    D2D1_EXTEND_MODE_CLAMP,
                ) else {
                    // TODO instead of panicking, I should just return a default value
                    panic!("could not create gradient_stop_collection!");
                };

                let Ok(brush) = render_target.CreateLinearGradientBrush(
                    &gradient_properties,
                    Some(brush_properties),
                    &gradient_stop_collection,
                ) else {
                    return None;
                };

                Some(brush.into())
            },
        }
    }
}

// Functions
fn is_valid_direction(direction: &str) -> bool {
    matches!(
        direction,
        "to right"
            | "to left"
            | "to top"
            | "to bottom"
            | "to top right"
            | "to top left"
            | "to bottom right"
            | "to bottom left"
    ) || direction
        .strip_suffix("deg")
        .and_then(|angle| angle.parse::<f32>().ok())
        .is_some()
}

pub fn interpolate_d2d1_colors(
    current_color: &D2D1_COLOR_F,
    start_color: &D2D1_COLOR_F,
    end_color: &D2D1_COLOR_F,
    anim_elapsed: f32,
    anim_speed: f32,
    finished: &mut bool,
) -> D2D1_COLOR_F {
    // D2D1_COLOR_F has the copy trait so we can just do this to create an implicit copy
    let mut interpolated = *current_color;

    let anim_step = anim_elapsed * anim_speed;

    let diff_r = end_color.r - start_color.r;
    let diff_g = end_color.g - start_color.g;
    let diff_b = end_color.b - start_color.b;
    let diff_a = end_color.a - start_color.a;

    interpolated.r += diff_r * anim_step;
    interpolated.g += diff_g * anim_step;
    interpolated.b += diff_b * anim_step;
    interpolated.a += diff_a * anim_step;

    // Check if we have overshot the active_color
    // TODO if I also check the alpha here, then things start to break when opening windows, not
    // sure why. Might be some sort of conflict with interpoalte_d2d1_to_visible().
    if (interpolated.r - end_color.r) * diff_r.signum() >= 0.0
        && (interpolated.g - end_color.g) * diff_g.signum() >= 0.0
        && (interpolated.b - end_color.b) * diff_b.signum() >= 0.0
    {
        *finished = true;
        return *end_color;
    } else {
        *finished = false;
    }

    interpolated
}

pub fn interpolate_d2d1_to_visible(
    current_color: &D2D1_COLOR_F,
    end_color: &D2D1_COLOR_F,
    anim_elapsed: f32,
    anim_speed: f32,
    finished: &mut bool,
) -> D2D1_COLOR_F {
    let mut interpolated = *current_color;

    let anim_step = anim_elapsed * anim_speed;

    // Figure out which direction we should be interpolating
    let diff = end_color.a - interpolated.a;
    match diff.is_sign_positive() {
        true => interpolated.a += anim_step,
        false => interpolated.a -= anim_step,
    }

    if (interpolated.a - end_color.a) * diff.signum() >= 0.0 {
        *finished = true;
        return *end_color;
    } else {
        *finished = false;
    }

    interpolated
}

pub fn interpolate_direction(
    current_direction: &GradientDirectionCoordinates,
    start_direction: &GradientDirectionCoordinates,
    end_direction: &GradientDirectionCoordinates,
    anim_elapsed: f32,
    anim_speed: f32,
) -> GradientDirectionCoordinates {
    let mut interpolated = (*current_direction).clone();

    let x_start_step = end_direction.start[0] - start_direction.start[0];
    let y_start_step = end_direction.start[1] - start_direction.start[1];
    let x_end_step = end_direction.end[0] - start_direction.end[0];
    let y_end_step = end_direction.end[1] - start_direction.end[1];

    // Not gonna bother checking if we overshot the direction tbh
    let anim_step = anim_elapsed * anim_speed;
    interpolated.start[0] += x_start_step * anim_step;
    interpolated.start[1] += y_start_step * anim_step;
    interpolated.end[0] += x_end_step * anim_step;
    interpolated.end[1] += y_end_step * anim_step;

    interpolated
}

fn interpolate_color(color1: D2D1_COLOR_F, color2: D2D1_COLOR_F, t: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: color1.r + t * (color2.r - color1.r),
        g: color1.g + t * (color2.g - color1.g),
        b: color1.b + t * (color2.b - color1.b),
        a: color1.a + t * (color2.a - color1.a),
    }
}

pub fn adjust_gradient_stops(
    source_stops: Vec<D2D1_GRADIENT_STOP>,
    target_count: usize,
) -> Vec<D2D1_GRADIENT_STOP> {
    if source_stops.len() == target_count {
        return source_stops;
    }

    let mut adjusted_stops = Vec::with_capacity(target_count);
    let step = 1.0 / (target_count - 1).max(1) as f32;

    for i in 0..target_count {
        let position = i as f32 * step;
        let (prev_stop, next_stop) = match source_stops
            .windows(2)
            .find(|w| w[0].position <= position && position <= w[1].position)
        {
            Some(pair) => (&pair[0], &pair[1]),
            None => {
                if position <= source_stops[0].position {
                    (&source_stops[0], &source_stops[0])
                } else {
                    let last = source_stops.last().unwrap();
                    (last, last)
                }
            }
        };

        let t = if prev_stop.position == next_stop.position {
            0.0
        } else {
            (position - prev_stop.position) / (next_stop.position - prev_stop.position)
        };

        let color = interpolate_color(prev_stop.color, next_stop.color, t);
        adjusted_stops.push(D2D1_GRADIENT_STOP { color, position });
    }

    adjusted_stops
}
