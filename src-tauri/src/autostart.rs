use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::config::ConfigState;
use crate::tray;

pub const STARTUP_TASK_ID: &str = "DesktopDashboardsStartup";

static CACHED_AUTOSTART_ENABLED: AtomicBool = AtomicBool::new(false);

/// OS 側の自動起動登録状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartOsState {
    Enabled,
    Disabled,
    DisabledByUser,
}

impl AutostartOsState {
    pub fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

pub fn get_cached_autostart() -> bool {
    CACHED_AUTOSTART_ENABLED.load(Ordering::SeqCst)
}

pub fn set_cached_autostart(enabled: bool) {
    CACHED_AUTOSTART_ENABLED.store(enabled, Ordering::SeqCst);
}

/// 現在のプロセスが MSIX 等のパッケージ環境として動作しているか判定する。
#[cfg(windows)]
pub fn is_packaged_app() -> bool {
    use windows::Win32::Foundation::{
        APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, WIN32_ERROR,
    };
    use windows::Win32::Storage::Packaging::Appx::GetCurrentPackageFullName;

    let mut len: u32 = 0;
    let err: WIN32_ERROR = unsafe { GetCurrentPackageFullName(&mut len, None) };
    if err == APPMODEL_ERROR_NO_PACKAGE {
        false
    } else {
        err == ERROR_INSUFFICIENT_BUFFER || (err == WIN32_ERROR(0) && len > 0)
    }
}

#[cfg(not(windows))]
pub fn is_packaged_app() -> bool {
    false
}

#[cfg(windows)]
struct ComGuard(bool);

#[cfg(windows)]
impl ComGuard {
    fn init_mta() -> Self {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        Self(hr.is_ok())
    }
}

#[cfg(windows)]
impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.0 {
            unsafe { windows::Win32::System::Com::CoUninitialize() };
        }
    }
}

#[cfg(windows)]
fn map_startup_task_state(state: windows::ApplicationModel::StartupTaskState) -> AutostartOsState {
    use windows::ApplicationModel::StartupTaskState;
    match state {
        StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy => AutostartOsState::Enabled,
        StartupTaskState::DisabledByUser | StartupTaskState::DisabledByPolicy => {
            AutostartOsState::DisabledByUser
        }
        _ => AutostartOsState::Disabled,
    }
}

#[cfg(windows)]
fn query_packaged_startup_state() -> Result<AutostartOsState, String> {
    use windows::core::HSTRING;
    use windows::ApplicationModel::StartupTask;

    let _com = ComGuard::init_mta();
    let task_id = HSTRING::from(STARTUP_TASK_ID);
    let task = StartupTask::GetAsync(&task_id)
        .map_err(|e| format!("StartupTask::GetAsync failed: {e}"))?
        .join()
        .map_err(|e| format!("StartupTask::GetAsync().join() failed: {e}"))?;
    let state = task
        .State()
        .map_err(|e| format!("StartupTask::State failed: {e}"))?;
    Ok(map_startup_task_state(state))
}

#[cfg(windows)]
fn set_packaged_startup_enabled(enabled: bool, lang: crate::i18n::Language) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::ApplicationModel::{StartupTask, StartupTaskState};

    let _com = ComGuard::init_mta();
    let task_id = HSTRING::from(STARTUP_TASK_ID);
    let task = StartupTask::GetAsync(&task_id)
        .map_err(|e| format!("StartupTask::GetAsync failed: {e}"))?
        .join()
        .map_err(|e| format!("StartupTask::GetAsync().join() failed: {e}"))?;

    if enabled {
        let current = task
            .State()
            .map_err(|e| format!("StartupTask::State failed: {e}"))?;
        match current {
            StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy => return Ok(()),
            StartupTaskState::DisabledByUser | StartupTaskState::DisabledByPolicy => {
                return Err(crate::i18n::err_autostart_disabled_by_user(lang).to_string());
            }
            _ => {}
        }

        let updated = task
            .RequestEnableAsync()
            .map_err(|e| format!("StartupTask::RequestEnableAsync failed: {e}"))?
            .join()
            .map_err(|e| format!("StartupTask::RequestEnableAsync().join() failed: {e}"))?;
        match updated {
            StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy => Ok(()),
            StartupTaskState::DisabledByUser | StartupTaskState::DisabledByPolicy => {
                Err(crate::i18n::err_autostart_disabled_by_user(lang).to_string())
            }
            _ => Err(crate::i18n::err_autostart_enable_failed(lang).to_string()),
        }
    } else {
        task.Disable()
            .map_err(|e| format!("StartupTask::Disable failed: {e}"))?;
        Ok(())
    }
}

/// 実行環境（MSIX / NSIS）に応じた OS 側の自動起動状態を取得する（ブロッキング呼び出し）。
pub fn query_os_autostart_state_blocking(app: &AppHandle) -> Result<AutostartOsState, String> {
    if is_packaged_app() {
        #[cfg(windows)]
        {
            return query_packaged_startup_state();
        }
        #[cfg(not(windows))]
        {
            return Ok(AutostartOsState::Disabled);
        }
    }

    let enabled = app
        .autolaunch()
        .is_enabled()
        .map_err(|e| format!("failed to query autostart: {e}"))?;
    Ok(if enabled {
        AutostartOsState::Enabled
    } else {
        AutostartOsState::Disabled
    })
}

/// 実行環境（MSIX / NSIS）に応じた OS 側の自動起動を設定する（ブロッキング呼び出し）。
pub fn set_os_autostart_enabled_blocking(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if is_packaged_app() {
        #[cfg(windows)]
        {
            let lang = crate::i18n::current_language(app);
            return set_packaged_startup_enabled(enabled, lang);
        }
        #[cfg(not(windows))]
        {
            let _ = enabled;
            return Ok(());
        }
    }

    if enabled {
        app.autolaunch()
            .enable()
            .map_err(|e| format!("failed to enable autostart: {e}"))
    } else {
        app.autolaunch()
            .disable()
            .map_err(|e| format!("failed to disable autostart: {e}"))
    }
}

/// `config.json` の自動起動設定と OS 側スタートアップ登録状態の差分を判定する。
/// 戻り値は `(OS側へ自動起動を再登録するか, config.json側をtrueへ同期保存するか)`。
pub fn plan_autostart_sync(config_autostart: bool, os_state: AutostartOsState) -> (bool, bool) {
    match (config_autostart, os_state) {
        (true, AutostartOsState::Disabled) => (true, false),
        // DisabledByUser は OS 側のユーザー意思を尊重し、強制再有効化も設定上書きもしない
        (true, AutostartOsState::DisabledByUser) => (false, false),
        (false, AutostartOsState::Enabled) => (false, true),
        _ => (false, false),
    }
}

/// 起動時に自動起動設定を非同期で照合し、必要なら OS 側登録を復元または config.json を同期する。
pub fn spawn_sync_autostart_on_startup(app: AppHandle, state: Arc<ConfigState>) {
    let config_autostart = state
        .config
        .lock()
        .map(|cfg| cfg.autostart)
        .unwrap_or(false);
    set_cached_autostart(config_autostart);

    tauri::async_runtime::spawn(async move {
        let app_for_query = app.clone();
        let os_state = match tauri::async_runtime::spawn_blocking(move || {
            query_os_autostart_state_blocking(&app_for_query)
        })
        .await
        {
            Ok(Ok(s)) => s,
            Ok(Err(e)) => {
                eprintln!("[autostart] failed to query OS autostart on startup: {e}");
                return;
            }
            Err(e) => {
                eprintln!("[autostart] startup query task join error: {e}");
                return;
            }
        };

        let (should_enable_os, should_save_config_true) =
            plan_autostart_sync(config_autostart, os_state);

        let mut effective_enabled = os_state.is_enabled();

        if should_enable_os {
            let app_for_enable = app.clone();
            match tauri::async_runtime::spawn_blocking(move || {
                set_os_autostart_enabled_blocking(&app_for_enable, true)
            })
            .await
            {
                Ok(Ok(())) => {
                    effective_enabled = true;
                }
                Ok(Err(e)) => {
                    eprintln!("[autostart] failed to restore OS autostart: {e}");
                }
                Err(e) => {
                    eprintln!("[autostart] restore task join error: {e}");
                }
            }
        }

        if should_save_config_true {
            if let Ok(mut cfg) = state.config.lock() {
                cfg.autostart = true;
            }
            let _ = state.save_now();
            effective_enabled = true;
        }

        set_cached_autostart(effective_enabled);
        tray::refresh_tray_menu(&app);
        let _ = app.emit("widgets-updated", ());
    });
}

/// 管理画面・トレイからの自動起動切替処理（OS 側反映 + config.json 更新 + トレイ更新）。
pub async fn apply_autostart_toggle(app: &AppHandle, enabled: bool) -> Result<bool, String> {
    let app_for_set = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        set_os_autostart_enabled_blocking(&app_for_set, enabled)
    })
    .await
    .map_err(|e| format!("autostart task join error: {e}"))??;

    if let Some(state) = app.try_state::<Arc<ConfigState>>() {
        if let Ok(mut cfg) = state.config.lock() {
            cfg.autostart = enabled;
        }
        let _ = state.save_now();
    }
    set_cached_autostart(enabled);
    tray::refresh_tray_menu(app);
    Ok(enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_autostart_sync_handles_all_states() {
        assert_eq!(
            plan_autostart_sync(true, AutostartOsState::Disabled),
            (true, false)
        );
        assert_eq!(
            plan_autostart_sync(true, AutostartOsState::DisabledByUser),
            (false, false)
        );
        assert_eq!(
            plan_autostart_sync(false, AutostartOsState::Enabled),
            (false, true)
        );
        assert_eq!(
            plan_autostart_sync(true, AutostartOsState::Enabled),
            (false, false)
        );
        assert_eq!(
            plan_autostart_sync(false, AutostartOsState::Disabled),
            (false, false)
        );
        assert_eq!(
            plan_autostart_sync(false, AutostartOsState::DisabledByUser),
            (false, false)
        );
    }

    #[test]
    fn test_is_packaged_app_false_in_test_runner() {
        assert!(!is_packaged_app());
    }
}
