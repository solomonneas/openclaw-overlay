mod database;
mod collectors;

use database::Database;
use std::sync::Arc;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Wry,
};

// ── Existing Commands ──

#[tauri::command]
async fn fetch_overlay_status() -> Result<String, String> {
    let url = "http://localhost:8005/api/overlay/status";
    let resp = reqwest::get(url)
        .await
        .map_err(|e| format!("fetch error: {}", e))?;
    let body = resp
        .text()
        .await
        .map_err(|e| format!("read error: {}", e))?;
    Ok(body)
}

#[tauri::command]
async fn win_close(window: tauri::WebviewWindow) -> Result<(), String> {
    window.close().map_err(|e| e.to_string())
}

#[tauri::command]
async fn win_minimize(window: tauri::WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
async fn win_start_drag(window: tauri::WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

// ── New Dashboard Commands ──

#[tauri::command]
async fn fetch_dashboard_data(
    db: tauri::State<'_, Arc<Database>>,
    days: Option<u32>,
) -> Result<serde_json::Value, String> {
    let days = days.unwrap_or(14);
    let mut result = db.get_dashboard_summary(days)?;

    // Add sessions list
    let sessions = db.get_sessions(days, "cost", 100)?;
    if let Some(obj) = result.as_object_mut() {
        obj.insert("sessions".to_string(), serde_json::Value::Array(sessions));
    }

    Ok(result)
}

#[tauri::command]
async fn fetch_rate_limit_history(
    db: tauri::State<'_, Arc<Database>>,
    provider: Option<String>,
    hours: Option<u32>,
) -> Result<Vec<serde_json::Value>, String> {
    let hours = hours.unwrap_or(24);
    db.get_rate_limit_history(provider.as_deref(), hours)
}

#[tauri::command]
async fn list_subscriptions(
    db: tauri::State<'_, Arc<Database>>,
) -> Result<Vec<serde_json::Value>, String> {
    db.list_subscriptions()
}

#[tauri::command]
async fn add_subscription(
    db: tauri::State<'_, Arc<Database>>,
    name: String,
    cost: f64,
    provider: Option<String>,
) -> Result<serde_json::Value, String> {
    db.add_subscription(&name, cost, provider.as_deref())
}

#[tauri::command]
async fn update_subscription(
    db: tauri::State<'_, Arc<Database>>,
    id: i64,
    name: Option<String>,
    cost: Option<f64>,
    enabled: Option<bool>,
) -> Result<(), String> {
    db.update_subscription(id, name.as_deref(), cost, enabled)
}

#[tauri::command]
async fn delete_subscription(
    db: tauri::State<'_, Arc<Database>>,
    id: i64,
) -> Result<(), String> {
    db.delete_subscription(id)
}

#[tauri::command]
async fn open_dashboard(app: tauri::AppHandle) -> Result<(), String> {
    show_dashboard_window(&app);
    Ok(())
}

// ── Helpers ──

fn show_dashboard_window(app: &tauri::AppHandle) {
    match app.get_webview_window("dashboard") {
        Some(window) => {
            log::info!("Showing existing dashboard window");
            window.show().ok();
            window.unminimize().ok();
            window.set_focus().ok();
        }
        None => {
            log::warn!("Dashboard window not found, attempting to create");
            if let Ok(window) = tauri::WebviewWindowBuilder::new(
                app,
                "dashboard",
                tauri::WebviewUrl::App("dashboard.html".into()),
            )
            .title("OpenClaw Usage Tracker")
            .inner_size(1200.0, 800.0)
            .min_inner_size(900.0, 600.0)
            .resizable(true)
            .build()
            {
                window.show().ok();
                window.set_focus().ok();
            }
        }
    }
}

fn show_overlay_window(app: &tauri::AppHandle) {
    match app.get_webview_window("overlay") {
        Some(window) => {
            log::info!("Showing overlay window");
            window.show().ok();
            window.unminimize().ok();
            window.set_focus().ok();
        }
        None => {
            log::warn!("Overlay window not found, attempting to create");
            if let Ok(window) = tauri::WebviewWindowBuilder::new(
                app,
                "overlay",
                tauri::WebviewUrl::App("overlay.html".into()),
            )
            .title("OpenClaw")
            .inner_size(320.0, 480.0)
            .always_on_top(true)
            .decorations(false)
            .build()
            {
                window.show().ok();
            }
        }
    }
}

// ── App Entry ──

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            // Existing
            fetch_overlay_status,
            win_close,
            win_minimize,
            win_start_drag,
            // New
            fetch_dashboard_data,
            fetch_rate_limit_history,
            list_subscriptions,
            add_subscription,
            update_subscription,
            delete_subscription,
            open_dashboard,
        ])
        .setup(|app| {
            // ── Initialize Database ──
            let db = Database::init().expect("Failed to initialize database");
            let db = Arc::new(db);
            app.manage(db.clone());

            // ── System Tray ──
            let show_overlay = MenuItem::with_id(app, "show", "Show Overlay", true, None::<&str>)?;
            let show_dashboard = MenuItem::with_id(app, "dashboard", "Open Dashboard", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_overlay, &show_dashboard, &separator, &quit])?;

            let tray_icon = Image::from_bytes(include_bytes!("../icons/tray-icon.png"))
                .expect("failed to load tray icon");

            TrayIconBuilder::new()
                .icon(tray_icon)
                .tooltip("OpenClaw Overlay")
                .menu(&menu)
                .on_menu_event(|app: &tauri::AppHandle<Wry>, event| match event.id.as_ref() {
                    "show" => {
                        show_overlay_window(app);
                    }
                    "dashboard" => {
                        show_dashboard_window(app);
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray: &tauri::tray::TrayIcon<Wry>, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        show_overlay_window(app);
                    }
                })
                .build(app)?;

            // ── Background Collectors ──
            let db_clone = db.clone();
            tauri::async_runtime::spawn(async move {
                // Initial collection after 5 second delay
                tokio::time::sleep(Duration::from_secs(5)).await;
                collectors::collect_rate_limits(&db_clone).await;
                collectors::collect_sessions(&db_clone).await;

                let mut tick_1m = tokio::time::interval(Duration::from_secs(60));
                let mut tick_5m = tokio::time::interval(Duration::from_secs(300));

                loop {
                    tokio::select! {
                        _ = tick_1m.tick() => {
                            collectors::collect_rate_limits(&db_clone).await;
                        }
                        _ = tick_5m.tick() => {
                            collectors::collect_sessions(&db_clone).await;
                        }
                    }
                }
            });

            // ── Logging (debug only) ──
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
