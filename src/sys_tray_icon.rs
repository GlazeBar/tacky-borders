use crate::border_config::Config;
use crate::keybinding::KeyBinding;
use crate::keybinding::KeyBindingHook;
// use crate::keybinding::{KeyBinding, KeyBindingHook};
use crate::reload_borders;
use crate::utils::get_config;
use crate::EVENT_HOOK;
use std::process::exit;
use tray_icon::menu::Menu;
use tray_icon::menu::MenuEvent;
use tray_icon::menu::MenuItem;
use tray_icon::Icon;
use tray_icon::TrayIcon;
use tray_icon::TrayIconBuilder;
// use win_binder::Key;
use win_hotkey::keys::VirtualKey;
use windows::Win32::System::Threading::ExitProcess;
use windows::Win32::UI::Accessibility::UnhookWinEvent;

fn reload_config() {
    Config::reload();
    reload_borders();
}

fn open_config() {
    let config_dir = get_config();
    let config_path = config_dir.join("config.yaml");
    let _ = open::that(config_path);
}

pub fn create_tray_icon() -> Result<TrayIcon, tray_icon::Error> {
    let icon = match Icon::from_resource(1, Some((64, 64))) {
        Ok(icon) => icon,
        Err(_) => {
            error!("could not retrieve tray icon!");
            exit(1);
        }
    };

    let tray_menu = Menu::new();
    let _ = tray_menu.append(&MenuItem::with_id("0", "Open Config", true, None));
    let _ = tray_menu.append(&MenuItem::with_id("1", "Reload", true, None));
    let _ = tray_menu.append(&MenuItem::with_id("2", "Close", true, None));

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_tooltip(format!("tacky-borders v{}", env!("CARGO_PKG_VERSION")))
        .with_icon(icon)
        .build();

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.0.as_str() {
        "0" => open_config(),
        "1" => reload_config(),
        "2" => unsafe {
            if UnhookWinEvent(EVENT_HOOK.get()).as_bool() {
                debug!("exiting tacky-borders!");
                ExitProcess(0);
            } else {
                error!("could not unhook win event hook");
            }
        },
        _ => {}
    }));

    tray_icon
}

pub fn bind_tray_hotkeys() {
    let keybinding_hook = KeyBindingHook::new(None);

    keybinding_hook.add_binding(KeyBinding::new(
        "reload".to_string(),
        VirtualKey::F8,
        None,
        Box::new(reload_config),
    ));

    keybinding_hook.add_binding(KeyBinding::new(
        "open_config".to_string(),
        VirtualKey::F9,
        None,
        Box::new(open_config),
    ));

    keybinding_hook.listen();
}
