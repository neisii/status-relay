use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Status {
    OPERATIONAL,
    DEGRADED,
    OUTAGE,
}

impl Status {
    fn from_raw(raw: &str) -> Self {
        let lower = raw.to_lowercase();
        if lower.contains("operational") {
            Status::OPERATIONAL
        } else if lower.contains("degraded") || lower.contains("minor") {
            Status::DEGRADED
        } else if lower.contains("outage") {
            Status::OUTAGE
        } else {
            Status::OUTAGE
        }
    }

    fn to_icon(&self) -> &'static str {
        match self {
            Status::OPERATIONAL => "🟢",
            Status::DEGRADED => "🟡",
            Status::OUTAGE => "🔴",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceState {
    pub service: String,
    pub status: Status,
    pub last_checked_at: u64,
    pub message: Option<String>,
}

#[derive(Clone)]
pub struct Config {
    pub polling_interval: u64,
    pub notifications_enabled: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            polling_interval: 60000,
            notifications_enabled: true,
        }
    }
}

pub struct AppState {
    pub states: Mutex<HashMap<String, ServiceState>>,
    pub config: Mutex<Config>,
}

#[derive(Deserialize, Debug)]
struct StatusPageResponse {
    status: StatusPageStatus,
}

#[derive(Deserialize, Debug)]
struct StatusPageStatus {
    description: String,
}

async fn fetch_status(url: &str) -> Result<Status, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::builder().user_agent("Mozilla/5.0 StatusRelay").build()?;
    let text = client.get(url).send().await?.text().await?;

    if let Ok(resp) = serde_json::from_str::<StatusPageResponse>(&text) {
        return Ok(Status::from_raw(&resp.status.description));
    }

    let lower = text.to_lowercase();
    if lower.contains("all systems operational") || lower.contains("not aware of any issues") || lower.contains("all services are online") {
        Ok(Status::OPERATIONAL)
    } else if lower.contains("minor service outage") || lower.contains("degraded performance") || lower.contains("partially degraded") {
        Ok(Status::DEGRADED)
    } else if lower.contains("major outage") || lower.contains("partial outage") || lower.contains("critical outage") || lower.contains("partial system outage") || lower.contains("major system outage") {
        Ok(Status::OUTAGE)
    } else {
        // Safe default to avoid false alarms when encountering unrecognized HTML, CDNs, or captchas
        Ok(Status::OPERATIONAL)
    }
}

const SERVICES: &[(&str, &str, &str)] = &[
    ("Claude", "https://status.claude.com/api/v2/status.json", "https://status.claude.com/"),
    ("Cloudflare", "https://www.cloudflarestatus.com/api/v2/status.json", "https://www.cloudflarestatus.com/"),
    ("Render", "https://status.render.com/api/v2/status.json", "https://status.render.com/"),
    ("Replit", "https://status.replit.com/", "https://status.replit.com/"),
    ("Supabase", "https://status.supabase.com/api/v2/status.json", "https://status.supabase.com/"),
    ("Vercel", "https://www.vercel-status.com/api/v2/status.json", "https://www.vercel-status.com/"),
    ("Netlify", "https://netlifystatus.com/api/v2/status.json", "https://netlifystatus.com/"),
    ("Railway", "https://status.railway.app/", "https://status.railway.app/"),
    ("Fly.io", "https://status.flyio.net/api/v2/status.json", "https://status.flyio.net/"),
];

async fn poll_services(app: AppHandle) {
    let state = app.state::<Arc<AppState>>();
    
    for (name, url, _) in SERVICES {
        match fetch_status(url).await {
            Ok(new_status) => {
                let mut states = state.states.lock().await;
                let prev_status = states.get(*name).map(|s| s.status.clone());
                
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                states.insert(name.to_string(), ServiceState {
                    service: name.to_string(),
                    status: new_status.clone(),
                    last_checked_at: now,
                    message: None,
                });

                let state_changed = match prev_status {
                    Some(ref prev) => prev != &new_status,
                    None => true, // initial state
                };

                if state_changed {
                    if let Some(prev) = prev_status {
                        notify_change(&app, *name, &prev, &new_status);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to fetch {} status: {}", name, e);
            }
        }
    }
    
    // Always update tray after a successful poll cycle
    // Note: If one fails, the tray updates with the last known state for that service
    let _ = update_tray_menu(&app).await;
}

fn notify_change(app: &AppHandle, service: &str, prev: &Status, next: &Status) {
    let title = match next {
        Status::OPERATIONAL => format!("{} recovered", service),
        Status::DEGRADED | Status::OUTAGE => format!("{} outage detected", service),
    };

    let body = format!("Status changed from {:?} to {:?}", prev, next);

    let _ = app.notification().builder()
        .title(title)
        .body(body)
        .show();
}

async fn update_tray_menu(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let state = app.state::<Arc<AppState>>();
    let states = state.states.lock().await;
    
    // Calculate worst status for main icon
    let mut worst_status = Status::OPERATIONAL;
    let mut any_outage = false;
    let mut any_degraded = false;

    for s in states.values() {
        match s.status {
            Status::OUTAGE => any_outage = true,
            Status::DEGRADED => any_degraded = true,
            _ => {}
        }
    }

    if any_outage {
        worst_status = Status::OUTAGE;
    } else if any_degraded {
        worst_status = Status::DEGRADED;
    }

    let main_icon_str = worst_status.to_icon();

    // Create a color badge (Green/Yellow/Red) based on the worst status
    let mut badged_icon = None;
    if let Some(base_icon) = app.default_window_icon() {
        let width = base_icon.width();
        let height = base_icon.height();
        let mut rgba = base_icon.rgba().to_vec();

        let (or, og, ob) = match worst_status {
            Status::OPERATIONAL => (52, 199, 89),
            Status::DEGRADED => (255, 204, 0),
            Status::OUTAGE => (255, 59, 48),
        };

        // Draw a circle in the bottom right corner
        let radius = (width as f32) / 4.5;
        let cx = (width as f32) - radius - (width as f32 * 0.05);
        let cy = (height as f32) - radius - (height as f32 * 0.05);

        for y in 0..height {
            for x in 0..width {
                let dx = (x as f32) - cx;
                let dy = (y as f32) - cy;
                let d = (dx * dx + dy * dy).sqrt();

                if d <= radius {
                    let mut alpha = 1.0;
                    if d > radius - 1.0 {
                        alpha = radius - d; // anti-aliasing
                    }
                    if alpha < 0.0 { alpha = 0.0; }

                    let offset = ((y * width + x) * 4) as usize;
                    let src_r = rgba[offset] as f32;
                    let src_g = rgba[offset + 1] as f32;
                    let src_b = rgba[offset + 2] as f32;
                    let src_a = rgba[offset + 3] as f32;

                    // Blend
                    let out_r = or as f32 * alpha + src_r * (1.0 - alpha);
                    let out_g = og as f32 * alpha + src_g * (1.0 - alpha);
                    let out_b = ob as f32 * alpha + src_b * (1.0 - alpha);
                    let out_a = src_a.max(255.0 * alpha);

                    rgba[offset] = out_r as u8;
                    rgba[offset + 1] = out_g as u8;
                    rgba[offset + 2] = out_b as u8;
                    rgba[offset + 3] = out_a as u8;
                }
            }
        }
        
        badged_icon = Some(tauri::image::Image::new(&rgba, width, height).to_owned());
    }

    // Build menu
    let mut menu_items = Vec::new();
    for (name, _, _) in SERVICES {
        let state = states.get(*name).map(|s| s.status.clone()).unwrap_or(Status::OPERATIONAL);
        let item = MenuItem::with_id(app, name.to_lowercase(), format!("{}: {}", name, state.to_icon()), true, None::<&str>)?;
        menu_items.push(item);
    }
    
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    // Convert to readable time string (simplified to local generic time representation or just UTC, let's keep it simple)
    use chrono::{TimeZone, Local};
    let time_str = Local.timestamp_opt(now as i64, 0).unwrap().format("%H:%M:%S").to_string();

    let update_item = MenuItem::with_id(app, "update", format!("Last Updated: {}", time_str), false, None::<&str>)?;
    let refresh_item = MenuItem::with_id(app, "refresh", "Refresh", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    let menu = Menu::new(app)?;
    for item in &menu_items {
        menu.append(item)?;
    }
    menu.append(&update_item)?;
    menu.append(&refresh_item)?;
    menu.append(&quit_item)?;

    if let Some(tray) = app.tray_by_id("main_tray") {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_title(None::<&str>);
        if let Some(img) = badged_icon {
            let _ = tray.set_icon(Some(img));
        }
    }

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app_state = Arc::new(AppState {
        states: Mutex::new(HashMap::new()),
        config: Mutex::new(Config::default()),
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(app_state.clone())
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            
            // Build initial tray
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit_i])?;
            
            let tray = TrayIconBuilder::with_id("main_tray")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .on_menu_event(|app, event| {
                    let id = event.id.as_ref();
                    if id == "quit" {
                        app.exit(0);
                    } else if id == "refresh" {
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            poll_services(handle).await;
                        });
                    } else {
                        for (name, _, web_url) in SERVICES {
                            if name.to_lowercase() == id {
                                use tauri_plugin_opener::OpenerExt;
                                let _ = app.opener().open_url(*web_url, None::<&str>);
                                break;
                            }
                        }
                    }
                })
                .build(app)?;

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    poll_services(handle.clone()).await;
                    
                        let state_ref = handle.state::<Arc<AppState>>();
                        let config = state_ref.config.lock().await;
                        let interval = config.polling_interval;
                        drop(config);
                        tokio::time::sleep(Duration::from_millis(interval)).await;
                }
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
