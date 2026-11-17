use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// 配置モード（グローバル設定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlacementMode {
    #[default]
    Bottom,
    WorkerW,
}

/// ガジェットのソース種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SourceType {
    #[default]
    Url,
    File,
}

/// 個別ガジェットの設定。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WidgetConfig {
    pub id: String,
    pub title: String,
    pub source_type: SourceType,
    pub source: String,
    pub enabled: bool,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub dpi_scale: f64,
    pub opacity: f64,
    pub zoom: f64,
    pub locked: bool,
    pub click_through: bool,
    pub no_activate: bool,
    pub auto_reload_minutes: u32,
    pub hide_scrollbar: bool,
    pub custom_css: String,
    pub placement_mode: PlacementMode,
}

impl Default for WidgetConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            title: "New Widget".to_string(),
            source_type: SourceType::Url,
            source: String::new(),
            enabled: true,
            x: 100,
            y: 100,
            width: 480,
            height: 760,
            dpi_scale: 1.0,
            opacity: 1.0,
            zoom: 1.0,
            locked: false,
            click_through: false,
            no_activate: false,
            auto_reload_minutes: 0,
            hide_scrollbar: false,
            custom_css: String::new(),
            placement_mode: PlacementMode::Bottom,
        }
    }
}

/// アプリ全体の設定 (`config.json`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub version: u32,
    pub placement_mode: PlacementMode,
    pub autostart: bool,
    pub widgets: Vec<WidgetConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: 1,
            placement_mode: PlacementMode::Bottom,
            autostart: false,
            widgets: Vec::new(),
        }
    }
}

/// 共有状態と原子的・デバウンス永続化を管理するストア。
pub struct ConfigState {
    pub config: Mutex<AppConfig>,
    pub config_path: PathBuf,
    save_generation: Arc<AtomicU64>,
}

impl ConfigState {
    /// 設定ファイルを読み込んでストアを初期化する（破損時は `.bak` へフォールバック）。
    pub fn load_or_init(app: &AppHandle) -> Result<Self, String> {
        let dir = app
            .path()
            .config_dir()
            .map_err(|e| format!("failed to resolve config_dir: {e}"))?
            .join("DesktopDashboards");
        fs::create_dir_all(&dir)
            .map_err(|e| format!("failed to create config dir {}: {e}", dir.display()))?;

        let config_path = dir.join("config.json");
        let is_first_launch = !config_path.exists() && !backup_path_for(&config_path).exists();
        let mut config = load_with_backup_fallback(&config_path);

        if is_first_launch && config.widgets.is_empty() {
            if let Some(default_widget) = crate::build_default_sample_widget(app) {
                config.widgets.push(default_widget);
                if let Err(e) = save_atomic_with_backup(&config_path, &config) {
                    eprintln!("[config] failed to persist default sample widget: {e}");
                }
            }
        }

        Ok(Self {
            config: Mutex::new(config),
            config_path,
            save_generation: Arc::new(AtomicU64::new(0)),
        })
    }

    /// 現在の設定を即時・原子的に保存する。
    pub fn save_now(&self) -> Result<(), String> {
        let snapshot = {
            let guard = self.config.lock().map_err(|e| format!("lock poisoned: {e}"))?;
            guard.clone()
        };
        save_atomic_with_backup(&self.config_path, &snapshot)
    }

    /// 位置・サイズ変更など高頻度更新向けにデバウンス（500ms）して保存する。
    pub fn schedule_debounced_save(state: &Arc<Self>) {
        let gen = state.save_generation.fetch_add(1, Ordering::SeqCst) + 1;
        let state_cloned = Arc::clone(state);

        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            if state_cloned.save_generation.load(Ordering::SeqCst) == gen {
                if let Err(e) = state_cloned.save_now() {
                    eprintln!("[ConfigState] debounced save failed: {e}");
                }
            }
        });
    }
}

/// `config.json` を読み込み、失敗した場合は `config.json.bak` へフォールバックする。
pub fn load_with_backup_fallback(config_path: &Path) -> AppConfig {
    if let Ok(text) = fs::read_to_string(config_path) {
        if let Ok(cfg) = serde_json::from_str::<AppConfig>(&text) {
            return cfg;
        }
        eprintln!(
            "[config] {} is corrupted, attempting backup fallback",
            config_path.display()
        );
    }

    let bak_path = backup_path_for(config_path);
    if let Ok(text) = fs::read_to_string(&bak_path) {
        if let Ok(cfg) = serde_json::from_str::<AppConfig>(&text) {
            eprintln!("[config] restored from {}", bak_path.display());
            return cfg;
        }
    }

    AppConfig::default()
}

/// `config.json.tmp` へ書き込んで `sync_all()` 後、既存を `.bak` に退避して原子的リネーム置換する。
pub fn save_atomic_with_backup(config_path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create dir {}: {e}", parent.display()))?;
    }

    let tmp_path = temp_path_for(config_path);
    let bak_path = backup_path_for(config_path);
    let json =
        serde_json::to_string_pretty(config).map_err(|e| format!("serialize error: {e}"))?;

    {
        let mut file = File::create(&tmp_path)
            .map_err(|e| format!("failed to create {}: {e}", tmp_path.display()))?;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("failed to write {}: {e}", tmp_path.display()))?;
        file.sync_all()
            .map_err(|e| format!("failed to sync {}: {e}", tmp_path.display()))?;
    }

    // 既存の正常ファイルを 1 世代バックアップとして退避
    if config_path.exists() {
        if let Ok(existing_text) = fs::read_to_string(config_path) {
            if serde_json::from_str::<AppConfig>(&existing_text).is_ok() {
                let _ = fs::copy(config_path, &bak_path);
            }
        }
    }

    // Windows では既存ファイルへの rename が上書き失敗する場合があるため、
    // 置換前に既存を削除するか、または一旦バックアップ済みなので置換する
    if config_path.exists() {
        fs::remove_file(config_path)
            .map_err(|e| format!("failed to remove old {}: {e}", config_path.display()))?;
    }
    fs::rename(&tmp_path, config_path)
        .map_err(|e| format!("failed to rename {} -> {}: {e}", tmp_path.display(), config_path.display()))?;

    Ok(())
}

fn temp_path_for(config_path: &Path) -> PathBuf {
    let mut name = config_path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    config_path.with_file_name(name)
}

fn backup_path_for(config_path: &Path) -> PathBuf {
    let mut name = config_path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    config_path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_save_and_backup_fallback() {
        let dir = std::env::temp_dir().join(format!("dd_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");

        let mut cfg1 = AppConfig::default();
        cfg1.widgets.push(WidgetConfig {
            title: "First".to_string(),
            source: "https://example.com".to_string(),
            ..Default::default()
        });
        save_atomic_with_backup(&cfg_path, &cfg1).unwrap();

        let mut cfg2 = cfg1.clone();
        cfg2.widgets[0].title = "Second".to_string();
        save_atomic_with_backup(&cfg_path, &cfg2).unwrap();

        // .bak には 1 世代前の "First" が残っている
        let loaded = load_with_backup_fallback(&cfg_path);
        assert_eq!(loaded.widgets[0].title, "Second");

        // config.json を破損させると .bak ("First") にフォールバックする
        fs::write(&cfg_path, "{ corrupted json !!!").unwrap();
        let recovered = load_with_backup_fallback(&cfg_path);
        assert_eq!(recovered.widgets[0].title, "First");

        let _ = fs::remove_dir_all(&dir);
    }
}
