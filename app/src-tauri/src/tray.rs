//! 系统托盘：显示主窗口、退出；左键单击还原窗口。

use crate::commands::window::{quit_app, restore_from_tray};
use crate::commands::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出程序", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    let icon = match app.default_window_icon() {
        Some(icon) => icon.clone(),
        None => tauri::image::Image::from_bytes(include_bytes!("../icons/32x32.png"))?,
    };

    let app_title = app
        .config()
        .app
        .windows
        .first()
        .map(|w| w.title.clone())
        .unwrap_or_else(|| "御钥师".to_string());

    let _tray = TrayIconBuilder::with_id("main-tray")
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(app_title)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => restore_from_tray(app),
            "quit" => {
                let state = app.state::<AppState>();
                quit_app(app, &state);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            }
            | TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                restore_from_tray(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
