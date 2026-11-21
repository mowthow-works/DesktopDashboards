use crate::autostart;
use crate::config::ConfigState;
use crate::widget;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, Wry};

const TRAY_ID: &str = "main-tray";

/// トレイの「終了」からの明示的終了かどうかを保持するフラグ。
pub struct AppQuitFlag(pub AtomicBool);

/// トレイメニューを構築する。
pub fn build_tray_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let mut builder = MenuBuilder::new(app);

    let open_item = MenuItemBuilder::with_id("open_manager", "管理画面を開く").build(app)?;
    builder = builder.item(&open_item).separator();

    let widgets = if let Some(state) = app.try_state::<Arc<ConfigState>>() {
        if let Ok(cfg) = state.config.lock() {
            cfg.widgets.clone()
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    if widgets.is_empty() {
        let empty_item = MenuItemBuilder::with_id("no_widgets", "(ガジェット未登録)")
            .enabled(false)
            .build(app)?;
        builder = builder.item(&empty_item);
    } else {
        for w in &widgets {
            let item = CheckMenuItemBuilder::with_id(format!("toggle:{}", w.id), &w.title)
                .checked(w.enabled)
                .build(app)?;
            builder = builder.item(&item);
        }
    }

    let autostart_enabled = autostart::get_cached_autostart();

    let lock_all_item = MenuItemBuilder::with_id("lock_all", "すべてロック").build(app)?;
    let unlock_all_item =
        MenuItemBuilder::with_id("unlock_all", "すべてロック解除").build(app)?;
    let reload_all_item =
        MenuItemBuilder::with_id("reload_all", "すべて再読み込み").build(app)?;
    let autostart_item =
        CheckMenuItemBuilder::with_id("autostart", "Windows起動時に自動起動")
            .checked(autostart_enabled)
            .build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "終了").build(app)?;

    builder
        .separator()
        .item(&lock_all_item)
        .item(&unlock_all_item)
        .item(&reload_all_item)
        .item(&autostart_item)
        .separator()
        .item(&quit_item)
        .build()
}

/// トレイメニューを最新状態に再構築して反映する。
pub fn refresh_tray_menu(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_tray_menu(app) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

/// システムトレイを初期化する。
pub fn init_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_tray_menu(app)?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("DesktopDashboards")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::DoubleClick {
                button: MouseButton::Left,
                ..
            } = event
            {
                crate::show_manager_window(tray.app_handle());
            }
        })
        .on_menu_event(|app, event| {
            let id = event.id().as_ref();
            if id == "open_manager" {
                crate::show_manager_window(app);
            } else if id == "lock_all" {
                let _ = crate::set_all_widgets_locked(app, true);
            } else if id == "unlock_all" {
                let _ = crate::set_all_widgets_locked(app, false);
            } else if id == "reload_all" {
                widget::reload_all_widget_windows(app);
            } else if id == "autostart" {
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let app_for_query = app_handle.clone();
                    let current = tauri::async_runtime::spawn_blocking(move || {
                        autostart::query_os_autostart_state_blocking(&app_for_query)
                            .map(|s| s.is_enabled())
                    })
                    .await
                    .ok()
                    .and_then(|r| r.ok())
                    .unwrap_or_else(autostart::get_cached_autostart);
                    let next = !current;
                    if let Err(e) = autostart::apply_autostart_toggle(&app_handle, next).await {
                        eprintln!("[tray] autostart toggle failed: {e}");
                        autostart::set_cached_autostart(current);
                        refresh_tray_menu(&app_handle);
                    } else {
                        let _ = app_handle.emit("widgets-updated", ());
                    }
                });
            } else if id == "quit" {
                if let Some(flag) = app.try_state::<AppQuitFlag>() {
                    flag.0.store(true, Ordering::SeqCst);
                }
                app.exit(0);
            } else if let Some(widget_id) = id.strip_prefix("toggle:") {
                let widget_id = widget_id.to_string();
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::toggle_widget_internal(app_handle, widget_id, None).await
                    {
                        eprintln!("[tray] toggle widget failed: {e}");
                    }
                });
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}
