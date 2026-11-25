use crate::config::ConfigState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// サポートする表示言語。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Ja,
    En,
}

impl Language {
    pub fn code(self) -> &'static str {
        match self {
            Language::Ja => "ja",
            Language::En => "en",
        }
    }
}

/// ホバー操作バーのラベル・ツールチップ文言。
#[derive(Debug, Clone, Serialize)]
pub struct HoverBarLabels {
    pub drag: &'static str,
    pub duplicate: &'static str,
    pub home: &'static str,
    pub reload: &'static str,
    pub lock: &'static str,
    pub close: &'static str,
}

/// 設定値 (`"auto"` / `"ja"` / `"en"`) を正規化する。未知の値は `"auto"` へフォールバック。
pub fn normalize_preference(raw: &str) -> &'static str {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "ja" => "ja",
        "en" => "en",
        _ => "auto",
    }
}

/// BCP-47 等のロケール文字列から `Language` を解決する（`ja*` -> `Ja`、その他 -> `En`）。
#[allow(dead_code)]
pub fn parse_locale_tag(tag: &str) -> Language {
    let lower = tag.trim().to_ascii_lowercase();
    if lower == "ja" || lower.starts_with("ja-") || lower.starts_with("ja_") {
        Language::Ja
    } else {
        Language::En
    }
}

/// Win32 LANGID からプライマリ言語 (`LANG_JAPANESE = 0x11`) を判定する。
pub fn lang_id_to_language(lang_id: u16) -> Language {
    const LANG_JAPANESE: u16 = 0x0011;
    if (lang_id & 0x03FF) == LANG_JAPANESE {
        Language::Ja
    } else {
        Language::En
    }
}

/// OS の表示言語を検出する（日本語環境は `Ja`、それ以外は `En`）。
pub fn detect_os_language() -> Language {
    #[cfg(windows)]
    {
        let lang_id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
        lang_id_to_language(lang_id)
    }
    #[cfg(not(windows))]
    {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(key) {
                if !val.trim().is_empty() {
                    return parse_locale_tag(&val);
                }
            }
        }
        Language::En
    }
}

/// 言語設定値 (`"auto"` / `"ja"` / `"en"`) から実効表示言語を解決する。
pub fn resolve_language(preference: &str) -> Language {
    match normalize_preference(preference) {
        "ja" => Language::Ja,
        "en" => Language::En,
        _ => detect_os_language(),
    }
}

/// `AppHandle` の `ConfigState` から現在の実効表示言語を取得する。
pub fn current_language(app: &AppHandle) -> Language {
    if let Some(state) = app.try_state::<Arc<ConfigState>>() {
        if let Ok(cfg) = state.config.lock() {
            return resolve_language(&cfg.language);
        }
    }
    detect_os_language()
}

pub fn hover_bar_labels(lang: Language) -> HoverBarLabels {
    match lang {
        Language::Ja => HoverBarLabels {
            drag: "⋮⋮ ドラッグで移動",
            duplicate: "新しいウィンドウを増やす",
            home: "ホームへ戻る",
            reload: "再読み込み",
            lock: "ロックする",
            close: "閉じる（非表示）",
        },
        Language::En => HoverBarLabels {
            drag: "⋮⋮ Drag to move",
            duplicate: "New window",
            home: "Home",
            reload: "Reload",
            lock: "Lock",
            close: "Close (Hide)",
        },
    }
}

pub fn manager_window_title(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "DesktopDashboards - 管理画面",
        Language::En => "DesktopDashboards - Manager",
    }
}

pub fn default_sticky_widget_title(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "付箋メモ＆ToDo",
        Language::En => "Sticky Notes & ToDo",
    }
}

pub fn tray_open_manager(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "管理画面を開く",
        Language::En => "Open Manager",
    }
}

pub fn tray_no_widgets(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "(ガジェット未登録)",
        Language::En => "(No widgets registered)",
    }
}

pub fn tray_lock_all(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "すべてロック",
        Language::En => "Lock All",
    }
}

pub fn tray_unlock_all(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "すべてロック解除",
        Language::En => "Unlock All",
    }
}

pub fn tray_reload_all(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "すべて再読み込み",
        Language::En => "Reload All",
    }
}

pub fn tray_autostart(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "Windows起動時に自動起動",
        Language::En => "Launch on Windows Startup",
    }
}

pub fn tray_quit(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "終了",
        Language::En => "Quit",
    }
}

pub fn err_autostart_disabled_by_user(lang: Language) -> &'static str {
    match lang {
        Language::Ja => {
            "Windows の「設定 > アプリ > スタートアップ」で無効化されているため、自動起動を有効にできません。設定アプリから有効にしてください。"
        }
        Language::En => {
            "Cannot enable startup because it is disabled in Windows Settings > Apps > Startup. Please enable it in Windows Settings."
        }
    }
}

pub fn err_autostart_enable_failed(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "Windows 側で自動起動を有効にできませんでした。",
        Language::En => "Windows could not enable startup automatically.",
    }
}

pub fn err_empty_source(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "URL またはファイルパスを入力してください",
        Language::En => "Please enter a URL or HTML file path",
    }
}

pub fn err_file_not_found(lang: Language, path: &str) -> String {
    match lang {
        Language::Ja => format!("ファイルが見つかりません: {path}"),
        Language::En => format!("File not found: {path}"),
    }
}

pub fn err_duplicate_url_not_supported(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "URL ウィジェットは増やせません（ローカル HTML のみ対応）",
        Language::En => "Cannot duplicate URL widgets (only local HTML widgets are supported)",
    }
}

pub fn err_duplicate_manifest_required(lang: Language) -> &'static str {
    match lang {
        Language::Ja => {
            "このウィジェットの manifest に \"duplicable\": true が宣言されていないため増やせません"
        }
        Language::En => {
            "This widget cannot be duplicated because \"duplicable\": true is not declared in its manifest"
        }
    }
}

pub fn err_conn_id_invalid(lang: Language) -> &'static str {
    match lang {
        Language::Ja => {
            "接続 ID は空欄不可で、英数字・ハイフン(-)・アンダースコア(_)のみ使用できます"
        }
        Language::En => {
            "Connection ID cannot be empty and may only contain letters, digits, hyphens (-), and underscores (_)"
        }
    }
}

pub fn err_dummy_token(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "ダミートークンは保存できません。実際のトークンを入力してください",
        Language::En => "Cannot save a placeholder token. Please enter your actual token",
    }
}

pub fn err_missing_required_credential(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "必須の認証情報（トークン等）を入力してください",
        Language::En => "Please enter the required credentials (e.g., API token)",
    }
}

pub fn err_credential_not_configured(
    lang: Language,
    connection: &str,
    cred_key: &str,
    target_name: &str,
) -> String {
    match lang {
        Language::Ja => format!(
            "接続 '{connection}' の認証情報 ({cred_key}) が Windows 資格情報マネージャーに未設定またはダミー値です (ターゲット: {target_name})"
        ),
        Language::En => format!(
            "Credential ({cred_key}) for connection '{connection}' is missing or still a placeholder in Windows Credential Manager (target: {target_name})"
        ),
    }
}

pub fn test_conn_no_cred_summary(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "認証情報未設定",
        Language::En => "Credentials not configured",
    }
}

pub fn test_conn_no_cred_detail(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "Windows 資格情報マネージャーにトークンが登録されていません。",
        Language::En => "No token is stored in Windows Credential Manager.",
    }
}

pub fn test_conn_no_cred_hint(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "「編集」からトークンを設定してください。",
        Language::En => "Click \"Edit\" to configure your token.",
    }
}

pub fn test_conn_net_err_summary(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "通信エラー",
        Language::En => "Network error",
    }
}

pub fn test_conn_net_err_hint(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "ネットワーク接続やプロキシ設定を確認してください。",
        Language::En => "Please check your network connection and proxy settings.",
    }
}

pub fn test_conn_success_summary(lang: Language, status: u16) -> String {
    match lang {
        Language::Ja => format!("接続成功 (HTTP {status})"),
        Language::En => format!("Connected (HTTP {status})"),
    }
}

pub fn test_conn_ok_default_detail(lang: Language) -> &'static str {
    match lang {
        Language::Ja => "疎通確認に成功しました。",
        Language::En => "Connectivity check succeeded.",
    }
}

pub fn test_conn_notion_bot_detail(lang: Language, name: &str, kind: &str) -> String {
    match lang {
        Language::Ja => format!("Bot名: {name} (type: {kind})"),
        Language::En => format!("Bot name: {name} (type: {kind})"),
    }
}

pub fn test_conn_notion_ok_hint(lang: Language) -> &'static str {
    match lang {
        Language::Ja => {
            "DB を参照するには、Notion 側の対象データベースの「⋯」→「コネクト」でこのインテグレーションを追加してください。"
        }
        Language::En => {
            "To access a database, open its \"⋯\" -> \"Connections\" menu in Notion and add this integration."
        }
    }
}

pub fn test_conn_fail_summary(lang: Language, status: u16) -> String {
    match lang {
        Language::Ja => format!("接続失敗 (HTTP {status})"),
        Language::En => format!("Connection failed (HTTP {status})"),
    }
}

pub fn test_conn_http_err_detail(lang: Language, status: u16) -> String {
    match lang {
        Language::Ja => format!("HTTP {status} エラーが返されました。"),
        Language::En => format!("Returned HTTP {status} error."),
    }
}

pub fn test_conn_hint_401(lang: Language) -> &'static str {
    match lang {
        Language::Ja => {
            "トークンが無効または取り消されている可能性があります。「編集」から正しいトークンに差し替えてください。"
        }
        Language::En => {
            "The token may be invalid or revoked. Use \"Edit\" to update it with a valid token."
        }
    }
}

pub fn test_conn_hint_404(lang: Language) -> &'static str {
    match lang {
        Language::Ja => {
            "対象のリソース／データベースが見つかりません。Notion 側の対象ページ・DB でこのインテグレーションが共有（コネクト追加）されているか確認してください。"
        }
        Language::En => {
            "Target resource or database was not found. Make sure this integration is shared via \"Connections\" on the Notion page/database."
        }
    }
}

pub fn test_conn_hint_403(lang: Language) -> &'static str {
    match lang {
        Language::Ja => {
            "インテグレーションの権限（Read content 等）が不足している可能性があります。"
        }
        Language::En => {
            "The integration may lack required capabilities (such as Read content)."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_locale_tag_resolves_ja_and_fallbacks_to_en() {
        assert_eq!(parse_locale_tag("ja"), Language::Ja);
        assert_eq!(parse_locale_tag("ja-JP"), Language::Ja);
        assert_eq!(parse_locale_tag("JA_jp"), Language::Ja);
        assert_eq!(parse_locale_tag("en-US"), Language::En);
        assert_eq!(parse_locale_tag("zh-CN"), Language::En);
        assert_eq!(parse_locale_tag(""), Language::En);
    }

    #[test]
    fn lang_id_to_language_maps_japanese_primary_lang() {
        assert_eq!(lang_id_to_language(0x0411), Language::Ja);
        assert_eq!(lang_id_to_language(0x0409), Language::En);
        assert_eq!(lang_id_to_language(0x0804), Language::En);
    }

    #[test]
    fn normalize_and_resolve_language_handles_auto_ja_en() {
        assert_eq!(normalize_preference("ja"), "ja");
        assert_eq!(normalize_preference(" EN "), "en");
        assert_eq!(normalize_preference("auto"), "auto");
        assert_eq!(normalize_preference("fr"), "auto");

        assert_eq!(resolve_language("ja"), Language::Ja);
        assert_eq!(resolve_language("en"), Language::En);
    }

    #[test]
    fn english_labels_are_populated() {
        let labels = hover_bar_labels(Language::En);
        assert!(labels.drag.contains("Drag to move"));
        assert_eq!(labels.duplicate, "New window");
        assert_eq!(tray_open_manager(Language::En), "Open Manager");
        assert_eq!(tray_quit(Language::En), "Quit");
        assert_eq!(
            default_sticky_widget_title(Language::En),
            "Sticky Notes & ToDo"
        );
    }
}
