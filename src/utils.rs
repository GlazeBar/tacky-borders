use dirs::home_dir;
use std::{
    fs::{self, DirBuilder, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use windows::{
    core::Error, Win32::Foundation::*, Win32::Graphics::Direct2D::Common::*,
    Win32::Graphics::Dwm::*, Win32::UI::WindowsAndMessaging::*,
};

use crate::*;

use crate::border_config::*;
use crate::logger::Logger;

#[derive(Debug, Clone)]
pub enum Color {
    Solid(D2D1_COLOR_F),
    Gradient(Gradient),
}

// Implement Default for your own MyBrush enum
impl Default for Color {
    fn default() -> Self {
        Color::Solid(D2D1_COLOR_F {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        })
    }
}

#[derive(Debug, Clone)]
pub struct Gradient {
    pub coordinates: Option<[f32; 4]>,
    pub direction: Option<String>, // Direction as a tuple (x, y) or start/end points
    pub gradient_stops: [D2D1_GRADIENT_STOP; 2], // Array of gradient stops
}

// Files
pub fn get_config() -> PathBuf {
    let home_dir = home_dir().expect("can't find home path");
    let config_dir = home_dir.join(".config").join("tacky-borders");
    let fallback_dir = home_dir.join(".tacky-borders");

    let dir_path = if fs::exists(&config_dir).expect("Couldn't check if config dir exists") {
        config_dir
    } else if fs::exists(&fallback_dir).expect("Couldn't check if config dir exists") {
        fallback_dir
    } else {
        DirBuilder::new()
            .recursive(true)
            .create(&config_dir)
            .expect("could not create config directory!");

        config_dir
    };

    dir_path
}

// Windows
pub fn get_width(rect: RECT) -> i32 {
    return rect.right - rect.left;
}

pub fn get_height(rect: RECT) -> i32 {
    return rect.bottom - rect.top;
}

pub fn is_window_visible(hwnd: HWND) -> bool {
    return unsafe { IsWindowVisible(hwnd).as_bool() };
}

pub fn has_filtered_style(hwnd: HWND) -> bool {
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) as u32 };
    let ex_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) as u32 };

    if style & WS_CHILD.0 != 0
        || ex_style & WS_EX_TOOLWINDOW.0 != 0
        || ex_style & WS_EX_NOACTIVATE.0 != 0
    {
        return true;
    }

    return false;
}

pub fn has_filtered_class_or_title(hwnd: HWND) -> bool {
    let mut class_arr: [u16; 256] = [0; 256];
    let mut title_arr: [u16; 256] = [0; 256];
    if unsafe { GetWindowTextW(hwnd, &mut title_arr) } == 0 {
        println!("error getting window title!");
        return true;
    }
    if unsafe { GetClassNameW(hwnd, &mut class_arr) } == 0 {
        println!("error getting class name!");
        return true;
    }

    let class_binding = String::from_utf16_lossy(&class_arr);
    let class_name = class_binding.split_once("\0").unwrap().0;

    let title_binding = String::from_utf16_lossy(&title_arr);
    let title = title_binding.split_once("\0").unwrap().0;

    let config_mutex = &*CONFIG;
    let config = config_mutex.lock().unwrap();

    let mut condition = false;

    for rule in config.window_rules.iter() {
        match rule.rule_match {
            RuleMatch::Global => {}
            RuleMatch::Title => {
                if let Some(contains_str) = &rule.contains {
                    if title.to_lowercase().contains(&contains_str.to_lowercase())
                        && rule.enabled == Some(false)
                    {
                        condition = true;
                        break;
                    }
                } else {
                    Logger::log("error", "Expected `contains` on `Match=\"Title\"`");
                }
            }

            RuleMatch::Class => {
                if let Some(contains_str) = &rule.contains {
                    if class_name
                        .to_lowercase()
                        .contains(&contains_str.to_lowercase())
                        && rule.enabled == Some(false)
                    {
                        condition = true;
                        break;
                    }
                } else {
                    Logger::log("error", "Expected `contains` on `Match=\"Class\"`");
                }
            }
        }
    }

    return condition;
}

pub fn is_cloaked(hwnd: HWND) -> bool {
    let mut is_cloaked = FALSE;
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            std::ptr::addr_of_mut!(is_cloaked) as *mut _,
            size_of::<BOOL>() as u32,
        )
    };
    if result.is_err() {
        println!("error getting is_cloaked");
        return true;
    }
    return is_cloaked.as_bool();
}

pub fn get_colors_for_window(_hwnd: HWND) -> (Color, Color) {
    let mut class_arr: [u16; 256] = [0; 256];
    let mut title_arr: [u16; 256] = [0; 256];
    if unsafe { GetWindowTextW(_hwnd, &mut title_arr) } == 0 {
        println!("error getting window title!");
    }
    if unsafe { GetClassNameW(_hwnd, &mut class_arr) } == 0 {
        println!("error getting class name!");
    }

    let class_binding = String::from_utf16_lossy(&class_arr);
    let class_name = class_binding.split_once("\0").unwrap().0;

    let title_binding = String::from_utf16_lossy(&title_arr);
    let title = title_binding.split_once("\0").unwrap().0;

    let config_mutex = &*CONFIG;
    let config = config_mutex.lock().unwrap();

    let mut color_active = Color::Solid(D2D1_COLOR_F {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    });
    let mut color_inactive = Color::Solid(D2D1_COLOR_F {
        r: 1.0,
        g: 1.0,
        b: 1.0,
        a: 1.0,
    });

    for rule in config.window_rules.iter() {
        match rule.rule_match {
            RuleMatch::Global => {
                color_active =
                    create_border_color(rule.active_color.as_deref().unwrap_or("accent"));
                color_inactive =
                    create_border_color(rule.inactive_color.as_deref().unwrap_or("accent"));
            }

            RuleMatch::Title => {
                if let Some(contains_str) = &rule.contains {
                    if title.to_lowercase().contains(&contains_str.to_lowercase()) {
                        color_active =
                            create_border_color(rule.active_color.as_deref().unwrap_or("accent"));
                        color_inactive =
                            create_border_color(rule.inactive_color.as_deref().unwrap_or("accent"));
                        break;
                    }
                } else {
                    Logger::log("error", "Expected `contains` on `Match=\"Title\"`");
                }
            }

            RuleMatch::Class => {
                if let Some(contains_str) = &rule.contains {
                    if class_name
                        .to_lowercase()
                        .contains(&contains_str.to_lowercase())
                    {
                        color_active =
                            create_border_color(rule.active_color.as_deref().unwrap_or("accent"));
                        color_inactive =
                            create_border_color(rule.inactive_color.as_deref().unwrap_or("accent"));
                        break;
                    }
                } else {
                    Logger::log("error", "Expected `contains` on `Match=\"Class\"`");
                }
            }
        }
    }

    (color_active, color_inactive)
}

pub fn create_border_for_window(tracking_window: HWND, delay: u64) -> Result<()> {
    let borders_mutex = &*BORDERS;
    let config_mutex = &*CONFIG;
    let window = SendHWND(tracking_window);

    let (active_color, inactive_color) = get_colors_for_window(tracking_window);

    let thread = std::thread::spawn(move || {
        let window_sent = window;

        // This delay can be used to wait for a window to finish its opening animation or for it to
        // become visible if it is not so at first
        std::thread::sleep(std::time::Duration::from_millis(delay));
        if unsafe { !IsWindowVisible(window_sent.0).as_bool() } {
            return;
        }

        let config = config_mutex.lock().unwrap();
        //println!("time it takes to get colors: {:?}", before.elapsed());

        let mut border = window_border::WindowBorder {
            tracking_window: window_sent.0,
            border_size: config.border_size,
            border_offset: config.border_offset,
            force_border_radius: config.get_border_radius(),
            active_color: active_color,
            inactive_color: inactive_color,
            ..Default::default()
        };
        drop(config);

        let mut borders_hashmap = borders_mutex.lock().unwrap();
        let window_isize = window_sent.0 .0 as isize;

        // Check to see if the key already exists in the hashmap. If not, then continue
        // adding the key and initializing the border. This is important because sometimes, the
        // event_hook function will call spawn_border_thread multiple times for the same window.
        if borders_hashmap.contains_key(&window_isize) {
            //println!("Duplicate window: {:?}", borders_hashmap);
            drop(borders_hashmap);
            return;
        }

        let hinstance: HINSTANCE = unsafe { std::mem::transmute(&__ImageBase) };
        let _ = border.create_border_window(hinstance);
        borders_hashmap.insert(window_isize, border.border_window.0 as isize);
        drop(borders_hashmap);

        let _ = border.init(hinstance);
    });

    return Ok(());
}

pub fn destroy_border_for_window(tracking_window: HWND) -> Result<()> {
    let mutex = &*BORDERS;
    let window = SendHWND(tracking_window);

    let _ = std::thread::spawn(move || {
        let window_sent = window;
        let mut borders_hashmap = mutex.lock().unwrap();
        let window_isize = window_sent.0 .0 as isize;
        let border_option = borders_hashmap.get(&window_isize);

        if border_option.is_some() {
            let border_window: HWND = HWND((*border_option.unwrap()) as *mut _);
            unsafe { SendMessageW(border_window, WM_DESTROY, WPARAM(0), LPARAM(0)) };
            borders_hashmap.remove(&window_isize);
        }

        drop(borders_hashmap);
    });

    return Ok(());
}

pub fn get_border_from_window(hwnd: HWND) -> Option<HWND> {
    let mutex = &*BORDERS;
    let borders = mutex.lock().unwrap();
    let hwnd_isize = hwnd.0 as isize;
    let border_option = borders.get(&hwnd_isize);

    if border_option.is_some() {
        let border_window: HWND = HWND(*border_option.unwrap() as _);
        drop(borders);
        return Some(border_window);
    } else {
        drop(borders);
        return None;
    }
}

// Return true if the border exists in the border hashmap. Otherwise, create a new border and
// return false.
// We can also specify a delay to prevent the border from appearing while a window is in its
// opening animation.
pub fn show_border_for_window(hwnd: HWND, delay: u64) -> bool {
    let border_window = get_border_from_window(hwnd);
    if border_window.is_some() {
        unsafe {
            let _ = ShowWindow(border_window.unwrap(), SW_SHOWNA);
        }
        return true;
    } else {
        if is_cloaked(hwnd) || has_filtered_style(hwnd) || has_filtered_class_or_title(hwnd) {
            return false;
        }
        let _ = create_border_for_window(hwnd, delay);
        return false;
    }
}

pub fn hide_border_for_window(hwnd: HWND) -> bool {
    let mutex = &*BORDERS;
    let window = SendHWND(hwnd);

    let _ = std::thread::spawn(move || {
        let window_sent = window;
        let borders = mutex.lock().unwrap();
        let window_isize = window_sent.0 .0 as isize;
        let border_option = borders.get(&window_isize);

        if border_option.is_some() {
            let border_window: HWND = HWND(*border_option.unwrap() as _);
            drop(borders);
            unsafe {
                let _ = ShowWindow(border_window, SW_HIDE);
            }
        } else {
            drop(borders);
        }
    });
    return true;
}

pub fn create_border_color(color: &str) -> Color {
    if color == "accent" {
        let mut pcr_colorization: u32 = 0;
        let mut pf_opaqueblend: BOOL = FALSE;
        let result = unsafe { DwmGetColorizationColor(&mut pcr_colorization, &mut pf_opaqueblend) };
        if result.is_err() {
            Logger::log("error", "Error getting windows accent color!");
            return Color::Solid(D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            });
        }
        let red = ((pcr_colorization & 0x00FF0000) >> 16) as f32 / 255.0;
        let green = ((pcr_colorization & 0x0000FF00) >> 8) as f32 / 255.0;
        let blue = ((pcr_colorization & 0x000000FF) >> 0) as f32 / 255.0;
        Color::Solid(D2D1_COLOR_F {
            r: red,
            g: green,
            b: blue,
            a: 1.0,
        })
    } else if (color.starts_with("rgb(")) || (color.starts_with("rgba(")) {
        Color::Solid(get_color_from_rgba(&color))
    } else if (color.starts_with("oklch(")) {
        Color::Solid(get_color_from_oklch(&color))
    } else if (color.starts_with("hsl(")) {
        Color::Solid(get_color_from_hsl(&color))
    } else if (color.starts_with("gradient(")) {
        Color::Gradient(get_gradient_color_from_string(&color))
    } else {
        Color::Solid(get_color_from_hex(&color))
    }
}

pub fn get_color_from_hex(hex: &str) -> D2D1_COLOR_F {
    // Ensure the hex string starts with '#' and is of the correct length
    if hex.len() != 7 && hex.len() != 9 && hex.len() != 4 && hex.len() != 5 || !hex.starts_with('#')
    {
        Logger::log(
            "error",
            format!("Invalid hex color format: {}", hex).as_str(),
        );
    }

    // Expand shorthand hex formats (#RGB or #RGBA to #RRGGBB or #RRGGBBAA)
    let expanded_hex = match hex.len() {
        4 => format!(
            "#{}{}{}{}{}{}",
            &hex[1..2],
            &hex[1..2],
            &hex[2..3],
            &hex[2..3],
            &hex[3..4],
            &hex[3..4]
        ),
        5 => format!(
            "#{}{}{}{}{}{}{}{}",
            &hex[1..2],
            &hex[1..2],
            &hex[2..3],
            &hex[2..3],
            &hex[3..4],
            &hex[3..4],
            &hex[4..5],
            &hex[4..5]
        ),
        _ => hex.to_string(),
    };

    // Convert each color component to f32 between 0.0 and 1.0, handling errors
    let parse_component = |s: &str| -> f32 {
        match u8::from_str_radix(s, 16) {
            Ok(val) => f32::from(val) / 255.0,
            Err(_) => {
                println!("Error: Invalid component '{}' in hex: {}", s, expanded_hex);
                0.0
            }
        }
    };

    // Parse RGB values
    let r = parse_component(&expanded_hex[1..3]);
    let g = parse_component(&expanded_hex[3..5]);
    let b = parse_component(&expanded_hex[5..7]);

    // Parse alpha value if present
    let a = if expanded_hex.len() == 9 {
        parse_component(&expanded_hex[7..9])
    } else {
        1.0
    };

    D2D1_COLOR_F { r, g, b, a }
}

pub fn get_color_from_rgba(rgba: &str) -> D2D1_COLOR_F {
    let rgba = rgba
        .trim_start_matches("rgb(")
        .trim_start_matches("rgba(")
        .trim_end_matches(')');
    let components: Vec<&str> = rgba.split(',').map(|s| s.trim()).collect();

    // Check for correct number of components
    if components.len() == 3 || components.len() == 4 {
        // Parse red, green, and blue values
        let red: f32 = f32::from_bits(components[0].parse::<u32>().unwrap_or(0)) / 255.0;
        let green: f32 = f32::from_bits(components[1].parse::<u32>().unwrap_or(0)) / 255.0;
        let blue: f32 = f32::from_bits(components[2].parse::<u32>().unwrap_or(0)) / 255.0;

        let alpha: f32 = if components.len() == 4 {
            components[3].parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0)
        } else {
            1.0
        };

        return D2D1_COLOR_F {
            r: red,
            g: green,
            b: blue,
            a: alpha, // Default alpha value for rgb()
        };
    }

    // Return a default color if parsing fails
    D2D1_COLOR_F {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    }
}

pub fn get_color_from_oklch(oklch: &str) -> D2D1_COLOR_F {
    let oklch = oklch.trim_start_matches("oklch(").trim_end_matches(')');
    let components: Vec<&str> = oklch.split(',').map(|s| s.trim()).collect(); // Split by commas

    // Check for the correct number of components (3)
    if components.len() == 3 {
        // Parse lightness, chroma, and hue values
        let lightness_str = components[0];
        let lightness: f64 = if lightness_str.ends_with('%') {
            lightness_str
                .trim_end_matches('%')
                .parse::<f64>()
                .unwrap_or(0.0)
                .clamp(0.0, 100.0)
                / 100.0 // Convert percentage to a 0.0 - 1.0 range
        } else {
            lightness_str.parse::<f64>().unwrap_or(0.0).clamp(0.0, 1.0) // Handle non-percentage case
        };

        let chroma: f64 = components[1]
            .parse::<f64>()
            .unwrap_or(0.0)
            .clamp(0.0, f64::MAX);
        let hue: f64 = components[2]
            .parse::<f64>()
            .unwrap_or(0.0)
            .clamp(0.0, 360.0);

        // Convert OKLCH to RGB
        let (r, g, b) = oklch_to_rgb(lightness, chroma, hue);

        return D2D1_COLOR_F {
            r: r as f32, // Convert back to f32 for D2D1_COLOR_F
            g: g as f32,
            b: b as f32,
            a: 1.0, // Default alpha value
        };
    }

    // Return a default color if parsing fails
    D2D1_COLOR_F {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    }
}

pub fn get_color_from_hsl(hsl: &str) -> D2D1_COLOR_F {
    let hsl = hsl.trim_start_matches("hsl(").trim_end_matches(')');
    let components: Vec<&str> = hsl.split(',').map(|s| s.trim()).collect(); // Split by commas

    // Check for the correct number of components (3)
    if components.len() == 3 {
        // Parse hue, saturation, and lightness values
        let hue: f64 = components[0]
            .parse::<f64>()
            .unwrap_or(0.0)
            .clamp(0.0, 360.0);

        let saturation_str = components[1];
        let saturation: f64 = if saturation_str.ends_with('%') {
            saturation_str
                .trim_end_matches('%')
                .parse::<f64>()
                .unwrap_or(0.0)
                .clamp(0.0, 100.0)
                / 100.0 // Convert percentage to a 0.0 - 1.0 range
        } else {
            saturation_str.parse::<f64>().unwrap_or(0.0).clamp(0.0, 1.0) // Handle non-percentage case
        };

        let lightness_str = components[2];
        let lightness: f64 = if lightness_str.ends_with('%') {
            lightness_str
                .trim_end_matches('%')
                .parse::<f64>()
                .unwrap_or(0.0)
                .clamp(0.0, 100.0)
                / 100.0 // Convert percentage to a 0.0 - 1.0 range
        } else {
            lightness_str.parse::<f64>().unwrap_or(0.0).clamp(0.0, 1.0) // Handle non-percentage case
        };

        // Convert HSL to RGB
        let (r, g, b) = hsl_to_rgb(hue, saturation, lightness);

        return D2D1_COLOR_F {
            r: r as f32, // Convert back to f32 for D2D1_COLOR_F
            g: g as f32,
            b: b as f32,
            a: 1.0, // Default alpha value
        };
    }

    // Return a default color if parsing fails
    D2D1_COLOR_F {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    }
}

// Color Functions
pub fn get_gradient_color_from_string(gradient_str: &str) -> Gradient {
    if !gradient_str.starts_with("gradient(") || !gradient_str.ends_with(')') {
        Logger::log(
            "error",
            format!("Invalid gradient format: {}", gradient_str).as_str(),
        );
    }

    let color_str = &gradient_str[9..gradient_str.len() - 1];

    let colors: Vec<&str> = color_str.split(',').collect();
    if colors.len() != 2 && colors.len() != 3 && colors.len() != 6 {
        Logger::log(
            "error",
            format!("Gradient must have exactly two colors, x coordinates, and y coordinates")
                .as_str(),
        );
    }

    let color1 = colors[0].trim();
    let color2 = colors[1].trim();

    let direction = match colors.len() {
        3 => Some(colors[2].trim().to_string()), // Use index 2 for the third element and wrap in Some
        2 => Some("to right".to_string()),
        _ => None,
    };

    let coordinates = match colors.len() {
        6 => {
            let start_x = convert_string_to_decimal(colors[2]).unwrap();
            let start_y = convert_string_to_decimal(colors[3]).unwrap();
            let end_x = convert_string_to_decimal(colors[4]).unwrap();
            let end_y = convert_string_to_decimal(colors[5]).unwrap();

            Some([start_x, start_y, end_x, end_y])
        }
        _ => None,
    };

    let gradients = Gradient {
        direction: direction,
        coordinates: coordinates,
        gradient_stops: [
            D2D1_GRADIENT_STOP {
                position: 0.0,
                color: get_color_from_hex(color1),
            },
            D2D1_GRADIENT_STOP {
                position: 1.0,
                color: get_color_from_hex(color2),
            },
        ],
    };

    gradients
}

// Converter
fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> (f64, f64, f64) {
    // Implement the conversion from HSL to RGB here
    // For now, returning a placeholder RGB value
    // This is just a placeholder; replace with actual conversion logic

    // HSL to RGB conversion logic
    let c = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation; // Chroma
    let x = c * (1.0 - ((hue / 60.0) % 2.0 - 1.0).abs()); // Second largest component
    let m = lightness - c / 2.0; // Match lightness

    let (r_prime, g_prime, b_prime) = match hue {
        h if h < 60.0 => (c, x, 0.0),
        h if h < 120.0 => (x, c, 0.0),
        h if h < 180.0 => (0.0, c, x),
        h if h < 240.0 => (0.0, x, c),
        h if h < 300.0 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };

    // Convert to RGB and apply match lightness
    let r = (r_prime + m).clamp(0.0, 1.0);
    let g = (g_prime + m).clamp(0.0, 1.0);
    let b = (b_prime + m).clamp(0.0, 1.0);

    (r, g, b)
}

// Placeholder for the actual OKLCH to RGB conversion function
fn oklch_to_rgb(lightness: f64, chroma: f64, hue: f64) -> (f64, f64, f64) {
    // Implement the conversion from OKLCH to RGB here
    // For now, returning a placeholder RGB value
    (lightness, chroma, hue) // This is just a placeholder; replace with actual conversion logic
}

fn convert_string_to_decimal(input: &str) -> Result<f32> {
    let trimmed = input.trim();

    // Check if the string represents a percentage
    if let Some(percent_str) = trimmed.strip_suffix("%") {
        let percent: f32 = percent_str
            .parse::<f32>()
            .map_err(|_| Error::new(HRESULT(0), "Invalid percentage"))?;
        if percent < 0.0 || percent > 100.0 {
            Logger::log("error", "Percentage out of range (0- 100)");
        }

        Ok(percent / 100.0)
    } else {
        // Assume it's a decimal string
        let value: f32 = trimmed
            .parse::<f32>()
            .map_err(|_| Error::new(HRESULT(0), "Invalid decimal"))?;

        // Ensure the decimal value is between 0 and 1
        if value < 0.0 || value > 1.0 {
            Logger::log("error", "Percentage out of range (0- 100)");
        }

        Ok(value) // Return the decimal value as is
    }
}
