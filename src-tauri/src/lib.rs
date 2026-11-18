mod cli;
mod config;
mod dd_bridge;
mod tray;
mod widget;
mod win32;

use config::{ConfigState, PlacementMode, SourceType, WidgetConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, RunEvent, Size,
    WebviewWindow, WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tray::AppQuitFlag;

/// 管理画面ウィンドウを前面表示する。
pub fn show_manager_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("manager") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

/// 2つの矩形が重なっているか（AABB 交差判定）と重複面積を計算する。
fn overlap_area(ax: i32, ay: i32, aw: i32, ah: i32, bx: i32, by: i32, bw: i32, bh: i32) -> i64 {
    let ix = (ax + aw).min(bx + bw) - ax.max(bx);
    let iy = (ay + ah).min(by + bh) - ay.max(by);
    if ix > 0 && iy > 0 {
        (ix as i64) * (iy as i64)
    } else {
        0
    }
}

/// 既存ガジェットと重ならない新規追加用の初期座標（非重複・空きスペース優先配置）を計算する。
fn next_non_overlapping_position(
    mon_x: i32,
    mon_y: i32,
    mon_w: i32,
    mon_h: i32,
    new_w: i32,
    new_h: i32,
    widgets: &[WidgetConfig],
) -> (i32, i32) {
    let margin = 40;
    let gap = 24;

    let mut x_candidates = vec![mon_x + margin];
    let mut y_candidates = vec![mon_y + margin];

    for w in widgets {
        x_candidates.push(w.x + w.width as i32 + gap);
        y_candidates.push(w.y + w.height as i32 + gap);
    }

    // グリッド候補も追加（空きスペースを細かく探索するため）
    let mut gx = mon_x + margin;
    while gx + new_w <= mon_x + mon_w - margin {
        x_candidates.push(gx);
        gx += 160;
    }
    let mut gy = mon_y + margin;
    while gy + new_h <= mon_y + mon_h - margin {
        y_candidates.push(gy);
        gy += 160;
    }

    x_candidates.sort_unstable();
    x_candidates.dedup();
    y_candidates.sort_unstable();
    y_candidates.dedup();

    let max_x = (mon_x + mon_w - new_w - margin).max(mon_x + margin);
    let max_y = (mon_y + mon_h - new_h - margin).max(mon_y + margin);

    let mut best_pos = (mon_x + margin, mon_y + margin);
    let mut best_overlap = i64::MAX;

    for &cy in &y_candidates {
        if cy < mon_y + margin || cy > max_y {
            continue;
        }
        for &cx in &x_candidates {
            if cx < mon_x + margin || cx > max_x {
                continue;
            }
            let total_overlap: i64 = widgets
                .iter()
                .filter(|w| w.enabled)
                .map(|w| {
                    overlap_area(
                        cx,
                        cy,
                        new_w,
                        new_h,
                        w.x,
                        w.y,
                        w.width as i32,
                        w.height as i32,
                    )
                })
                .sum();

            if total_overlap == 0 {
                return (cx, cy);
            }
            if total_overlap < best_overlap {
                best_overlap = total_overlap;
                best_pos = (cx, cy);
            }
        }
    }

    best_pos
}

fn next_cascade_position(app: &AppHandle, widgets: &[WidgetConfig]) -> (i32, i32) {
    let default_w = 480;
    let default_h = 760;
    let (mx, my, mw, mh) = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| {
            (
                m.position().x,
                m.position().y,
                m.size().width as i32,
                m.size().height as i32,
            )
        })
        .unwrap_or((0, 0, 1920, 1080));
    next_non_overlapping_position(mx, my, mw, mh, default_w, default_h, widgets)
}

/// `--add-url` で指定された URL を追加または既存を有効化して表示する。
pub async fn add_or_activate_url_widget(
    app: AppHandle,
    url: String,
    title: Option<String>,
) -> Result<WidgetConfig, String> {
    let parsed = url::Url::parse(&url).map_err(|e| format!("invalid URL '{url}': {e}"))?;
    let normalized = parsed.to_string();

    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;

    let target_widget = {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;

        if let Some(existing) = cfg
            .widgets
            .iter_mut()
            .find(|w| w.source_type == SourceType::Url && w.source == normalized)
        {
            existing.enabled = true;
            if let Some(t) = title {
                if !t.trim().is_empty() {
                    existing.title = t;
                }
            }
            existing.clone()
        } else {
            let derived_title = title
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| parsed.host_str().unwrap_or("Web Widget").to_string());
            let (x, y) = next_cascade_position(&app, &cfg.widgets);
            let new_widget = WidgetConfig {
                title: derived_title,
                source_type: SourceType::Url,
                source: normalized,
                enabled: true,
                x,
                y,
                ..Default::default()
            };
            cfg.widgets.push(new_widget.clone());
            new_widget
        }
    };

    state.save_now()?;
    widget::create_widget_window(app.clone(), target_widget.clone()).await?;
    tray::refresh_tray_menu(&app);
    let _ = app.emit("widgets-updated", ());
    Ok(target_widget)
}

/// ガジェットの表示/非表示を切り替える共通処理（コマンド・トレイ両用）。
pub async fn toggle_widget_internal(
    app: AppHandle,
    id: String,
    explicit_enabled: Option<bool>,
) -> Result<(), String> {
    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;

    let updated = {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;
        let Some(w) = cfg.widgets.iter_mut().find(|w| w.id == id) else {
            return Err(format!("widget not found: {id}"));
        };
        w.enabled = explicit_enabled.unwrap_or(!w.enabled);
        w.clone()
    };

    state.save_now()?;
    if updated.enabled {
        widget::create_widget_window(app.clone(), updated).await?;
    } else {
        widget::close_widget_window(&app, &id);
    }
    tray::refresh_tray_menu(&app);
    let _ = app.emit("widgets-updated", ());
    Ok(())
}

/// 全ガジェットのロック状態を一括変更する（トレイメニュー用）。
pub fn set_all_widgets_locked(app: &AppHandle, locked: bool) -> Result<(), String> {
    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;

    let updated_widgets = {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;
        for w in &mut cfg.widgets {
            w.locked = locked;
        }
        cfg.widgets.clone()
    };

    state.save_now()?;
    for w in &updated_widgets {
        if w.enabled {
            widget::apply_runtime_settings(app, w);
        }
    }
    let _ = app.emit("widgets-updated", ());
    Ok(())
}

/// 起動時または 2 つ目のプロセスから受け取った CLI オプションを処理する。
fn handle_cli_options(app: &AppHandle, opts: cli::CliOptions, is_secondary_instance: bool) {
    if opts.open_manager
        || (!is_secondary_instance && !opts.silent)
        || (is_secondary_instance && opts.add_url.is_none() && !opts.silent)
    {
        show_manager_window(app);
    }

    if let Some(url) = opts.add_url {
        let app_handle = app.clone();
        let title = opts.title;
        tauri::async_runtime::spawn(async move {
            if let Err(e) = add_or_activate_url_widget(app_handle, url, title).await {
                eprintln!("[cli] failed to add url widget: {e}");
            }
        });
    }
}

fn strip_windows_verbatim_prefix(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// 開発時リポジトリ直下の `samples/` ディレクトリを返す。
fn dev_samples_dir() -> Option<std::path::PathBuf> {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|d| d.join("samples"))
        .filter(|d| d.is_dir())
}

/// 同梱サンプル HTML フォルダを解決する（インストール版リソース → 実行ファイル隣 → 開発時リポジトリ直下）。
pub(crate) fn resolve_samples_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("samples"));
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            candidates.push(exe_dir.join("samples"));
        }
    }
    if let Some(dev_dir) = dev_samples_dir() {
        candidates.push(dev_dir);
    }
    candidates.into_iter().find(|p| p.is_dir())
}

/// 指定された `samples/` ディレクトリ内の `sticky-todo.html` から初期デフォルトガジェットを生成する。
pub fn default_sample_widget_for_dir(
    samples_dir: &std::path::Path,
    x: i32,
    y: i32,
) -> Option<WidgetConfig> {
    let sticky_path = samples_dir.join("sticky-todo.html");
    if !sticky_path.is_file() {
        return None;
    }
    Some(WidgetConfig {
        title: "付箋メモ＆ToDo".to_string(),
        source_type: SourceType::File,
        source: strip_windows_verbatim_prefix(&sticky_path),
        enabled: true,
        x,
        y,
        width: 400,
        height: 580,
        ..Default::default()
    })
}

/// 初回起動時（`config.json` 未作成時）にデフォルト登録するサンプルガジェットを構築する。
pub fn build_default_sample_widget(app: &AppHandle) -> Option<WidgetConfig> {
    let samples_dir = resolve_samples_dir(app)?;
    let (x, y) = next_cascade_position(app, &[]);
    default_sample_widget_for_dir(&samples_dir, x, y)
}

#[tauri::command]
fn get_webview_data_dir(app: AppHandle) -> Result<String, String> {
    let dir = widget::resolve_webview_data_dir(&app)?;
    Ok(dir.to_string_lossy().into_owned())
}

#[tauri::command]
fn get_samples_dir(app: AppHandle) -> Result<Option<String>, String> {
    Ok(resolve_samples_dir(&app).map(|p| strip_windows_verbatim_prefix(&p)))
}

#[tauri::command]
fn list_widgets(app: AppHandle) -> Result<Vec<WidgetConfig>, String> {
    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("config lock poisoned: {e}"))?;
    Ok(cfg.widgets.clone())
}

#[tauri::command]
async fn add_widget(
    app: AppHandle,
    source_type: SourceType,
    source: String,
    title: String,
) -> Result<WidgetConfig, String> {
    let trimmed_source = source.trim().to_string();
    if trimmed_source.is_empty() {
        return Err("URL またはファイルパスを入力してください".to_string());
    }

    if source_type == SourceType::Url {
        return add_or_activate_url_widget(app, trimmed_source, Some(title)).await;
    }

    let path = std::path::Path::new(&trimmed_source);
    if !path.is_file() {
        return Err(format!("ファイルが見つかりません: {trimmed_source}"));
    }

    let derived_title = if title.trim().is_empty() {
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Local HTML")
            .to_string()
    } else {
        title.trim().to_string()
    };

    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;

    let new_widget = {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;
        let (x, y) = next_cascade_position(&app, &cfg.widgets);
        let w = WidgetConfig {
            title: derived_title,
            source_type: SourceType::File,
            source: trimmed_source,
            enabled: true,
            x,
            y,
            ..Default::default()
        };
        cfg.widgets.push(w.clone());
        w
    };

    state.save_now()?;
    widget::create_widget_window(app.clone(), new_widget.clone()).await?;
    tray::refresh_tray_menu(&app);
    let _ = app.emit("widgets-updated", ());
    Ok(new_widget)
}

#[tauri::command]
async fn toggle_widget(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    toggle_widget_internal(app, id, Some(enabled)).await
}

#[tauri::command]
async fn update_widget_bounds(
    app: AppHandle,
    id: String,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
) -> Result<(), String> {
    let clamped_w = width.max(120);
    let clamped_h = height.max(80);
    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;

    {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;
        let Some(w) = cfg.widgets.iter_mut().find(|w| w.id == id) else {
            return Err(format!("widget not found: {id}"));
        };
        w.x = x;
        w.y = y;
        w.width = clamped_w;
        w.height = clamped_h;
    }

    state.save_now()?;
    if let Some(win) = app.get_webview_window(&widget::widget_label(&id)) {
        let _ = win.set_position(Position::Physical(PhysicalPosition::new(x, y)));
        let _ = win.set_size(Size::Physical(PhysicalSize::new(clamped_w, clamped_h)));
    }
    let _ = app.emit("widgets-updated", ());
    Ok(())
}

#[tauri::command]
async fn update_widget_settings(
    app: AppHandle,
    id: String,
    placement_mode: PlacementMode,
    locked: bool,
    click_through: bool,
    no_activate: bool,
    opacity: f64,
    zoom: f64,
    auto_reload_minutes: u32,
    custom_css: String,
) -> Result<(), String> {
    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;

    let updated = {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;
        let Some(w) = cfg.widgets.iter_mut().find(|w| w.id == id) else {
            return Err(format!("widget not found: {id}"));
        };
        w.placement_mode = placement_mode;
        w.locked = locked;
        w.click_through = click_through;
        w.no_activate = no_activate;
        w.opacity = opacity.clamp(0.2, 1.0);
        w.zoom = zoom.clamp(0.5, 2.0);
        w.auto_reload_minutes = auto_reload_minutes;
        w.custom_css = custom_css;
        w.clone()
    };

    state.save_now()?;
    if updated.enabled {
        widget::apply_runtime_settings(&app, &updated);
    }
    let _ = app.emit("widgets-updated", ());
    Ok(())
}

#[tauri::command]
async fn widget_bar_action(
    app: AppHandle,
    window: WebviewWindow,
    action: String,
) -> Result<(), String> {
    let Some(widget_id) = window.label().strip_prefix("widget-").map(|s| s.to_string()) else {
        return Err("invalid widget window".to_string());
    };

    match action.as_str() {
        "home" => {
            let state = app
                .try_state::<Arc<ConfigState>>()
                .ok_or_else(|| "ConfigState not initialized".to_string())?;
            let target = {
                let cfg = state
                    .config
                    .lock()
                    .map_err(|e| format!("config lock poisoned: {e}"))?;
                cfg.widgets.iter().find(|w| w.id == widget_id).cloned()
            };
            if let Some(w) = target {
                if let Ok(tauri::WebviewUrl::External(url)) = widget::build_webview_url(&w) {
                    let _ = window.navigate(url);
                }
            }
        }
        "reload" => {
            let _ = window.reload();
        }
        "lock" => {
            let state = app
                .try_state::<Arc<ConfigState>>()
                .ok_or_else(|| "ConfigState not initialized".to_string())?;
            let updated = {
                let mut cfg = state
                    .config
                    .lock()
                    .map_err(|e| format!("config lock poisoned: {e}"))?;
                let Some(w) = cfg.widgets.iter_mut().find(|w| w.id == widget_id) else {
                    return Err(format!("widget not found: {widget_id}"));
                };
                w.locked = true;
                w.clone()
            };
            state.save_now()?;
            widget::apply_runtime_settings(&app, &updated);
            let _ = app.emit("widgets-updated", ());
        }
        "close" => {
            toggle_widget_internal(app, widget_id, Some(false)).await?;
        }
        _ => return Err(format!("unknown action: {action}")),
    }

    Ok(())
}

#[tauri::command]
async fn reset_widget_position(app: AppHandle, id: String) -> Result<(), String> {
    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;

    let (center_x, center_y) = if let Ok(Some(monitor)) = app.primary_monitor() {
        let m_pos = monitor.position();
        let m_size = monitor.size();
        (
            m_pos.x + ((m_size.width as i32 - 480) / 2).max(40),
            m_pos.y + ((m_size.height as i32 - 760) / 2).max(40),
        )
    } else {
        (120, 80)
    };

    {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;
        let Some(w) = cfg.widgets.iter_mut().find(|w| w.id == id) else {
            return Err(format!("widget not found: {id}"));
        };
        w.x = center_x;
        w.y = center_y;
    }

    state.save_now()?;
    if let Some(win) = app.get_webview_window(&widget::widget_label(&id)) {
        let _ = win.set_position(Position::Physical(PhysicalPosition::new(center_x, center_y)));
    }
    let _ = app.emit("widgets-updated", ());
    Ok(())
}

#[tauri::command]
fn delete_widget(app: AppHandle, id: String) -> Result<(), String> {
    widget::close_widget_window(&app, &id);
    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;

    {
        let mut cfg = state
            .config
            .lock()
            .map_err(|e| format!("config lock poisoned: {e}"))?;
        cfg.widgets.retain(|w| w.id != id);
    }

    state.save_now()?;
    tray::refresh_tray_menu(&app);
    let _ = app.emit("widgets-updated", ());
    Ok(())
}

#[tauri::command]
fn reload_widget(app: AppHandle, id: String) {
    widget::reload_widget_window(&app, &id);
}

#[tauri::command]
fn reload_all_widgets(app: AppHandle) {
    widget::reload_all_widget_windows(&app);
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("failed to query autostart: {e}"))
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    if enabled {
        app.autolaunch()
            .enable()
            .map_err(|e| format!("failed to enable autostart: {e}"))?;
    } else {
        app.autolaunch()
            .disable()
            .map_err(|e| format!("failed to disable autostart: {e}"))?;
    }

    if let Some(state) = app.try_state::<Arc<ConfigState>>() {
        if let Ok(mut cfg) = state.config.lock() {
            cfg.autostart = enabled;
        }
        let _ = state.save_now();
    }
    tray::refresh_tray_menu(&app);
    Ok(enabled)
}

/// `config.json` の自動起動設定と OS 側スタートアップ登録状態の差分を判定する。
/// 戻り値は `(OS側へ自動起動を再登録するか, config.json側をtrueへ同期保存するか)`。
pub fn plan_autostart_sync(config_autostart: bool, os_enabled: bool) -> (bool, bool) {
    match (config_autostart, os_enabled) {
        (true, false) => (true, false),
        (false, true) => (false, true),
        _ => (false, false),
    }
}

/// 起動時に自動起動設定を照合し、インストーラ上書き更新等で OS 側登録が消えていた場合は自動復元する。
fn sync_autostart_on_startup(app: &AppHandle, state: &Arc<ConfigState>) {
    let config_autostart = state
        .config
        .lock()
        .map(|cfg| cfg.autostart)
        .unwrap_or(false);
    let autolaunch = app.autolaunch();
    let os_enabled = autolaunch.is_enabled().unwrap_or(false);

    let (should_enable_os, should_save_config_true) =
        plan_autostart_sync(config_autostart, os_enabled);

    if should_enable_os {
        if let Err(e) = autolaunch.enable() {
            eprintln!("[autostart] failed to restore OS autostart: {e}");
        }
    }

    if should_save_config_true {
        if let Ok(mut cfg) = state.config.lock() {
            cfg.autostart = true;
        }
        let _ = state.save_now();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let args = if argv.len() > 1 { &argv[1..] } else { &[] };
            let opts = cli::parse_args(args);
            handle_cli_options(app, opts, true);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--silent"]),
        ))
        .register_uri_scheme_protocol("widgetfile", |ctx, request| {
            widget::handle_widgetfile_protocol(ctx.app_handle(), request)
        })
        .setup(|app| {
            app.manage(AppQuitFlag(AtomicBool::new(false)));

            let config_state = Arc::new(
                ConfigState::load_or_init(app.handle())
                    .expect("failed to initialize ConfigState"),
            );
            sync_autostart_on_startup(app.handle(), &config_state);
            app.manage(config_state);

            if let Err(e) = dd_bridge::migrate_and_ensure_connections_for_app(app.handle()) {
                eprintln!("[dd_bridge] failed to initialize or migrate connections.json: {e}");
            }

            if let Some(manager_win) = app.get_webview_window("manager") {
                let win_clone = manager_win.clone();
                manager_win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_clone.hide();
                    }
                });
            }

            tray::init_tray(app.handle())?;
            widget::start_auto_reload_loop(app.handle().clone());
            widget::start_workerw_watcher(app.handle().clone());

            let args: Vec<String> = std::env::args().skip(1).collect();
            let opts = cli::parse_args(&args);
            let app_handle = app.handle().clone();

            tauri::async_runtime::spawn(async move {
                widget::sync_all_widgets(app_handle).await;
            });

            handle_cli_options(app.handle(), opts, false);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_webview_data_dir,
            get_samples_dir,
            list_widgets,
            add_widget,
            toggle_widget,
            update_widget_bounds,
            update_widget_settings,
            widget_bar_action,
            reset_widget_position,
            delete_widget,
            reload_widget,
            reload_all_widgets,
            get_autostart,
            set_autostart,
            dd_bridge::dd_storage_get,
            dd_bridge::dd_storage_set,
            dd_bridge::dd_connect_request,
        ])
        .build(tauri::generate_context!())
        .expect("error while building DesktopDashboards");

    app.run(|app_handle, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            let should_quit = app_handle
                .try_state::<AppQuitFlag>()
                .map(|f| f.0.load(Ordering::SeqCst))
                .unwrap_or(false);
            if !should_quit {
                api.prevent_exit();
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cascade_position_avoids_overlap() {
        let (x1, y1) = next_non_overlapping_position(0, 0, 1920, 1080, 480, 760, &[]);
        let first = WidgetConfig {
            x: x1,
            y: y1,
            width: 480,
            height: 760,
            enabled: true,
            ..Default::default()
        };
        let (x2, y2) =
            next_non_overlapping_position(0, 0, 1920, 1080, 480, 760, &[first.clone()]);
        assert_eq!(overlap_area(x1, y1, 480, 760, x2, y2, 480, 760), 0);

        let second = WidgetConfig {
            x: x2,
            y: y2,
            width: 480,
            height: 760,
            enabled: true,
            ..Default::default()
        };
        let (x3, y3) =
            next_non_overlapping_position(0, 0, 1920, 1080, 480, 760, &[first, second]);
        assert_eq!(overlap_area(x1, y1, 480, 760, x3, y3, 480, 760), 0);
        assert_eq!(overlap_area(x2, y2, 480, 760, x3, y3, 480, 760), 0);
    }

    #[test]
    fn bundled_samples_exist_and_prefix_is_stripped() {
        let samples_dir = dev_samples_dir().expect("samples directory should exist at repo root");
        for name in [
            "sticky-todo.html",
            "pomodoro-timer.html",
            "starter-template.html",
        ] {
            assert!(
                samples_dir.join(name).is_file(),
                "missing bundled sample file: {name}"
            );
        }

        let verbatim = std::path::Path::new(r"\\?\C:\Program Files\DesktopDashboards\samples");
        assert_eq!(
            strip_windows_verbatim_prefix(verbatim),
            r"C:\Program Files\DesktopDashboards\samples"
        );
    }

    #[test]
    fn test_plan_autostart_sync() {
        assert_eq!(plan_autostart_sync(true, false), (true, false));
        assert_eq!(plan_autostart_sync(false, true), (false, true));
        assert_eq!(plan_autostart_sync(true, true), (false, false));
        assert_eq!(plan_autostart_sync(false, false), (false, false));
    }

    #[test]
    fn default_sample_widget_uses_sticky_todo() {
        let samples_dir = dev_samples_dir().expect("samples directory should exist at repo root");
        let w = default_sample_widget_for_dir(&samples_dir, 40, 40)
            .expect("default sample widget should be created");
        assert_eq!(w.title, "付箋メモ＆ToDo");
        assert_eq!(w.source_type, SourceType::File);
        assert!(w.source.ends_with("sticky-todo.html"));
        assert!(w.enabled);
        assert_eq!((w.width, w.height), (400, 580));
    }
}
