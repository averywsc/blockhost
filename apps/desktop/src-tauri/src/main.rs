#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use blockhost_core::{events::StatusSnapshot, manager::Manager};
use std::time::Duration;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager as TauriManager, RunEvent, State, WebviewUrl, WebviewWindowBuilder,
    WindowEvent,
};

struct AppState {
    core: Manager,
}

#[tauri::command]
fn get_status(state: State<'_, AppState>, selected: Option<String>) -> StatusSnapshot {
    state.core.status(selected.as_deref())
}

#[tauri::command]
fn ping_core(state: State<'_, AppState>) -> StatusSnapshot {
    state.core.log("[blockhost] UI connected — core heartbeat OK");
    state.core.emit_status(None);
    state.core.status(None)
}

/// Show the main window, recreating the WebView2 instance if it was destroyed on close-to-tray.
/// Destroying the WebView when hidden frees ~100+ MB of RAM on 4 GB laptops running 24/7.
fn show_or_recreate_main_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }

    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("BlockHost")
        .inner_size(1024.0, 700.0)
        .min_inner_size(820.0, 560.0)
        .center()
        .build();
}

fn main() {
    let core = Manager::new();
    let core_for_events = core.clone();
    let core_for_exit = core.clone();

    let app = tauri::Builder::default()
        .manage(AppState { core })
        .invoke_handler(tauri::generate_handler![get_status, ping_core])
        .setup(move |app| {
            let open_item = MenuItem::with_id(app, "open", "Open BlockHost", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit BlockHost", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &quit_item])?;

            let mut tray = TrayIconBuilder::with_id("blockhost-tray")
                .menu(&menu)
                .tooltip("BlockHost — Minecraft Server Host")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => show_or_recreate_main_window(app),
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_or_recreate_main_window(tray.app_handle());
                    }
                });

            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            tray.build(app)?;

            // Forward typed core events to any open WebView window.
            let handle = app.handle().clone();
            let mut rx = core_for_events.subscribe();
            tauri::async_runtime::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            let _ = handle.emit("core://event", &ev);
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    }
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window destroys the WebView to save RAM while keeping BlockHost alive in the tray.
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { .. } = event {
                    // Let the window close naturally; RunEvent::ExitRequested keeps the tray process alive.
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building BlockHost");

    app.run(move |_app_handle, event| match event {
        RunEvent::ExitRequested { api, code, .. } => {
            // If code is None, the last window closed -> keep running in tray with zero WebView RAM.
            // If code is Some(_), the user explicitly chose Quit from the tray menu.
            if code.is_none() {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => {
            core_for_exit.shutdown_and_wait(Duration::from_secs(60));
        }
        _ => {}
    });
}
