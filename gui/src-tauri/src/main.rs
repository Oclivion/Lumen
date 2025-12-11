//! Lumen GUI - Tauri-based system tray application for Cardano node management
//!
//! Provides a lightweight GUI with:
//! - System tray icon with quick actions
//! - Dashboard showing node status
//! - First-run setup wizard
//! - Update notifications

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::Mutex;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State,
};

/// Application state
struct AppState {
    node_running: Mutex<bool>,
    network: Mutex<String>,
    sync_progress: Mutex<f64>,
}

/// Node status information
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeStatus {
    running: bool,
    network: String,
    sync_progress: f64,
    tip_epoch: Option<u32>,
    tip_slot: Option<u64>,
    peers: Option<u32>,
    memory_mb: Option<u64>,
    uptime_secs: Option<u64>,
}

impl Default for NodeStatus {
    fn default() -> Self {
        Self {
            running: false,
            network: "mainnet".to_string(),
            sync_progress: 0.0,
            tip_epoch: None,
            tip_slot: None,
            peers: None,
            memory_mb: None,
            uptime_secs: None,
        }
    }
}

/// Get current node status by calling the orchestrator CLI
#[tauri::command]
async fn get_status() -> Result<NodeStatus, String> {
    let output = Command::new("lumen")
        .args(["status", "--json"])
        .output()
        .map_err(|e| format!("Failed to execute lumen: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        // Try to parse JSON, fall back to basic status
        if let Ok(status) = serde_json::from_str::<NodeStatus>(&stdout) {
            return Ok(status);
        }
    }

    // Fallback: parse text output
    let stdout = String::from_utf8_lossy(&output.stdout);
    let running = stdout.contains("Running");

    Ok(NodeStatus {
        running,
        ..Default::default()
    })
}

/// Start the Cardano node
#[tauri::command]
async fn start_node(state: State<'_, AppState>, network: String) -> Result<String, String> {
    let output = Command::new("lumen")
        .args(["--network", &network, "start"])
        .output()
        .map_err(|e| format!("Failed to start node: {}", e))?;

    if output.status.success() {
        *state.node_running.lock().unwrap() = true;
        *state.network.lock().unwrap() = network;
        Ok("Node started successfully".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to start node: {}", stderr))
    }
}

/// Stop the Cardano node
#[tauri::command]
async fn stop_node(state: State<'_, AppState>) -> Result<String, String> {
    let output = Command::new("lumen")
        .args(["stop"])
        .output()
        .map_err(|e| format!("Failed to stop node: {}", e))?;

    if output.status.success() {
        *state.node_running.lock().unwrap() = false;
        Ok("Node stopped successfully".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to stop node: {}", stderr))
    }
}

/// Check for updates
#[tauri::command]
async fn check_updates() -> Result<Option<String>, String> {
    let output = Command::new("lumen")
        .args(["update", "--check"])
        .output()
        .map_err(|e| format!("Failed to check updates: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    if stdout.contains("Update available") {
        // Extract version from output
        for line in stdout.lines() {
            if line.contains("Update available:") {
                return Ok(Some(line.to_string()));
            }
        }
        Ok(Some("Update available".to_string()))
    } else {
        Ok(None)
    }
}

/// Apply update
#[tauri::command]
async fn apply_update() -> Result<String, String> {
    let output = Command::new("lumen")
        .args(["update"])
        .output()
        .map_err(|e| format!("Failed to apply update: {}", e))?;

    if output.status.success() {
        Ok("Update applied successfully. Please restart Lumen.".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to apply update: {}", stderr))
    }
}

/// Download Mithril snapshot for fast sync
#[tauri::command]
async fn download_mithril(network: String) -> Result<String, String> {
    let output = Command::new("lumen")
        .args(["--network", &network, "mithril", "download"])
        .output()
        .map_err(|e| format!("Failed to download snapshot: {}", e))?;

    if output.status.success() {
        Ok("Snapshot downloaded successfully".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to download snapshot: {}", stderr))
    }
}

/// Get available Mithril snapshots
#[tauri::command]
async fn list_snapshots(network: String) -> Result<Vec<String>, String> {
    let output = Command::new("lumen")
        .args(["--network", &network, "mithril", "list"])
        .output()
        .map_err(|e| format!("Failed to list snapshots: {}", e))?;

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let snapshots: Vec<String> = stdout
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.contains("INFO"))
            .map(|l| l.to_string())
            .collect();
        Ok(snapshots)
    } else {
        Err("Failed to list snapshots".to_string())
    }
}

/// Initialize configuration
#[tauri::command]
async fn init_config(network: String, data_dir: Option<String>) -> Result<String, String> {
    let mut args = vec!["--network".to_string(), network, "init".to_string()];

    if let Some(dir) = data_dir {
        args.insert(0, "--data-dir".to_string());
        args.insert(1, dir);
    }

    let output = Command::new("lumen")
        .args(&args)
        .output()
        .map_err(|e| format!("Failed to initialize: {}", e))?;

    if output.status.success() {
        Ok("Configuration initialized".to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("Failed to initialize: {}", stderr))
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState {
            node_running: Mutex::new(false),
            network: Mutex::new("mainnet".to_string()),
            sync_progress: Mutex::new(0.0),
        })
        .setup(|app| {
            // Create system tray menu
            let quit = MenuItem::with_id(app, "quit", "Quit Lumen", true, None::<&str>)?;
            let show = MenuItem::with_id(app, "show", "Show Dashboard", true, None::<&str>)?;
            let start = MenuItem::with_id(app, "start", "Start Node", true, None::<&str>)?;
            let stop = MenuItem::with_id(app, "stop", "Stop Node", true, None::<&str>)?;
            let status = MenuItem::with_id(app, "status", "Status: Stopped", false, None::<&str>)?;

            let menu = Menu::with_items(app, &[&status, &show, &start, &stop, &quit])?;

            // Create tray icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("Lumen - Cardano Node")
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "quit" => {
                            // Stop node before quitting
                            let _ = Command::new("lumen").args(["stop"]).output();
                            app.exit(0);
                        }
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "start" => {
                            let _ = Command::new("lumen").args(["start"]).spawn();
                        }
                        "stop" => {
                            let _ = Command::new("lumen").args(["stop"]).spawn();
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            start_node,
            stop_node,
            check_updates,
            apply_update,
            download_mithril,
            list_snapshots,
            init_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
