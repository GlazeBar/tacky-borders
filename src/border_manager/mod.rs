mod border;

use crate::error::LogIfErr;
use crate::windows_api::SendHWND;
use crate::windows_api::WindowsApi;
use crate::windows_api::WM_APP_HIDECLOAKED;
use crate::windows_api::WM_APP_SHOWUNCLOAKED;
use anyhow::Context;
use anyhow::Result as AnyResult;
pub use border::Border;
use rustc_hash::FxHashMap;
use std::mem::transmute;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::thread::spawn;
use windows::core::w;
use windows::Win32::Foundation::GetLastError;
use windows::Win32::Foundation::HINSTANCE;
use windows::Win32::Foundation::HWND;
use windows::Win32::Foundation::LPARAM;
use windows::Win32::Foundation::WPARAM;
use windows::Win32::System::SystemServices::IMAGE_DOS_HEADER;
use windows::Win32::UI::WindowsAndMessaging::LoadCursorW;
use windows::Win32::UI::WindowsAndMessaging::RegisterClassW;
use windows::Win32::UI::WindowsAndMessaging::IDC_ARROW;
use windows::Win32::UI::WindowsAndMessaging::WM_NCDESTROY;
use windows::Win32::UI::WindowsAndMessaging::WNDCLASSW;

extern "C" {
    static __ImageBase: IMAGE_DOS_HEADER;
}

static BORDERS: LazyLock<Mutex<FxHashMap<isize, isize>>> =
    LazyLock::new(|| Mutex::new(FxHashMap::default()));

pub fn get_border_from_window(hwnd: HWND) -> Option<HWND> {
    let borders = get_borders();
    let hwnd_isize = hwnd.0 as isize;
    let Some(border_isize) = borders.get(&hwnd_isize) else {
        drop(borders);
        return None;
    };

    let border_window: HWND = HWND(*border_isize as _);
    drop(borders);
    Some(border_window)
}

pub fn show_border_for_window(hwnd: HWND) {
    // If the border already exists, simply post a 'SHOW' message to its message queue. Otherwise,
    // create a new border.
    if let Some(border) = get_border_from_window(hwnd) {
        WindowsApi::post_message_w(border, WM_APP_SHOWUNCLOAKED, WPARAM(0), LPARAM(0))
            .context("show_border_for_window")
            .log_if_err();
    } else if WindowsApi::is_window_visible(hwnd)
        && !WindowsApi::is_window_cloaked(hwnd)
        && !WindowsApi::has_filtered_style(hwnd)
    {
        create_border_for_window(hwnd);
    }
}

pub fn get_borders() -> MutexGuard<'static, FxHashMap<isize, isize>> {
    BORDERS.lock().unwrap()
}

pub fn hide_border_for_window(hwnd: HWND) -> bool {
    let window = SendHWND(hwnd);

    let _ = spawn(move || {
        let window_sent = window;
        if let Some(border) = get_border_from_window(window_sent.0) {
            WindowsApi::post_message_w(border, WM_APP_HIDECLOAKED, WPARAM(0), LPARAM(0))
                .context("hide_border_for_window")
                .log_if_err();
        }
    });
    true
}

pub fn create_border_for_window(hwnd: HWND) {
    if !WindowsApi::is_window_visible(hwnd) || WindowsApi::is_window_cloaked(hwnd) {
        return;
    }
    debug!("creating border for: {:?}", hwnd);
    let window = SendHWND(hwnd);

    let _ = std::thread::spawn(move || {
        let window_sent = window;
        let window_isize = window_sent.0 .0 as isize;

        let window_rule = WindowsApi::get_window_rule(window_sent.0);
        if window_rule.rule_match.border_enabled == Some(false) {
            info!("border is disabled for {:?}!", window_sent.0);
            return;
        }

        let mut border = Border::new(window_sent.0, &window_rule);

        let mut borders_hashmap = get_borders();

        // Check to see if there is already a border for the given tracking window
        if borders_hashmap.contains_key(&window_isize) {
            return;
        }

        let hinstance: HINSTANCE = unsafe { std::mem::transmute(&__ImageBase) };
        if let Err(e) = border.create_border_window(hinstance) {
            error!("could not create border window: {e:?}");
            return;
        };

        borders_hashmap.insert(window_isize, border.border_window.0 as isize);

        drop(borders_hashmap);
        let _ = window_sent;
        let _ = window_isize;
        let _ = window_rule;
        let _ = hinstance;

        if let Err(e) = border.init() {
            error!("{e}");
        }
    });
}

pub fn destroy_border_for_window(hwnd: HWND) {
    let Some(&border_isize) = get_borders().get(&(hwnd.0 as isize)) else {
        return;
    };

    let border_window: HWND = HWND(border_isize as _);
    WindowsApi::post_message_w(border_window, WM_NCDESTROY, WPARAM(0), LPARAM(0))
        .context("destroy_border_for_window")
        .log_if_err();
}

pub fn register_border_class() -> AnyResult<()> {
    unsafe {
        let hinstance: HINSTANCE = transmute(&__ImageBase);

        let wc = WNDCLASSW {
            lpfnWndProc: Some(Border::wnd_proc),
            hInstance: hinstance,
            lpszClassName: w!("border"),
            hCursor: LoadCursorW(None, IDC_ARROW)?,
            ..Default::default()
        };

        let result = RegisterClassW(&wc);
        if result == 0 {
            let last_error = GetLastError();
            error!("could not register window border class: {last_error:?}");
        }
    }

    Ok(())
}

pub fn reload_borders() {
    let mut borders = get_borders();
    for value in borders.values() {
        let border_window = HWND(*value as _);
        WindowsApi::post_message_w(border_window, WM_NCDESTROY, WPARAM(0), LPARAM(0))
            .context("reload_borders")
            .log_if_err();
    }

    // Clear the borders hashmap
    borders.clear();
    drop(borders);

    WindowsApi::available_window_handles(Some(&create_border_for_window)).unwrap();
}
