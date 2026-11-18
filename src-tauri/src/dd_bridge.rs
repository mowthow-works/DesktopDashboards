use crate::config::{ConfigState, SourceType, WidgetConfig};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewWindow};

const FORBIDDEN_URL_WIDGET_ERR: &str =
    "DD bridge is forbidden for URL widgets (SourceType::File required)";
const DUMMY_NOTION_TOKEN_ERR: &str =
    "connections.json の notion.token がダミー値または未設定です。%APPDATA%\\DesktopDashboards\\connections.json にトークンを設定してください";
const DEFAULT_NOTION_BASE_URL: &str = "https://api.notion.com";
const DEFAULT_NOTION_DUMMY_TOKEN: &str = "ntn_DUMMY_REPLACE_WITH_YOUR_NOTION_TOKEN";
const DEFAULT_NOTION_VERSION: &str = "2025-09-03";

static STORAGE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn storage_lock() -> &'static Mutex<()> {
    STORAGE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    })
}

#[derive(Deserialize)]
pub struct ConnectRequest {
    pub connection: String,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub body: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct ConnectResponse {
    pub status: u16,
    pub ok: bool,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionsFile {
    pub version: u32,
    pub connections: BTreeMap<String, ConnectionEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectionEntry {
    pub base_url: String,
    pub token: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

impl fmt::Debug for ConnectionEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionEntry")
            .field("base_url", &self.base_url)
            .field("token", &"[REDACTED]")
            .field("headers", &self.headers)
            .finish()
    }
}

fn validate_widget_id(widget_id: &str) -> Result<(), String> {
    if widget_id.is_empty()
        || widget_id.contains("..")
        || widget_id.contains('/')
        || widget_id.contains('\\')
        || widget_id.contains('\0')
        || widget_id.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err("invalid widget_id".to_string());
    }
    let mut components = Path::new(widget_id).components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(()),
        _ => Err("invalid widget_id".to_string()),
    }
}

/// 呼び出し元ウィンドウが登録済みのローカル HTML ガジェット (`SourceType::File`) であることを検証する。
pub fn verify_file_widget_caller(
    window_label: &str,
    widgets: &[WidgetConfig],
) -> Result<String, String> {
    let widget_id = window_label
        .strip_prefix("widget-")
        .ok_or_else(|| format!("DD bridge is only allowed for widget windows (got '{window_label}')"))?;
    validate_widget_id(widget_id)?;

    let widget = widgets
        .iter()
        .find(|w| w.id == widget_id)
        .ok_or_else(|| format!("widget not found: {widget_id}"))?;

    if widget.source_type != SourceType::File {
        return Err(FORBIDDEN_URL_WIDGET_ERR.to_string());
    }

    Ok(widget_id.to_string())
}

/// IPC 呼び出し元の `Origin` ヘッダが `http://widgetfile.localhost`（ポート指定なし）であることを強制検証する。
pub fn verify_widgetfile_origin(origin_header: Option<&str>) -> Result<(), String> {
    let Some(raw) = origin_header else {
        return Err(
            "DD bridge is forbidden for non-widgetfile origin (missing Origin header)".to_string(),
        );
    };
    let parsed = url::Url::parse(raw).map_err(|_| {
        format!("DD bridge is forbidden for non-widgetfile origin (invalid Origin: '{raw}')")
    })?;
    if parsed.scheme() == "http"
        && parsed.host_str() == Some("widgetfile.localhost")
        && parsed.port().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && (parsed.path().is_empty() || parsed.path() == "/")
        && parsed.query().is_none()
        && parsed.fragment().is_none()
    {
        Ok(())
    } else {
        Err(format!(
            "DD bridge is forbidden for non-widgetfile origin (got '{raw}')"
        ))
    }
}

/// 呼び出し元ウィンドウ（`SourceType::File`）と IPC `Origin` ヘッダ（`http://widgetfile.localhost`）の双方を検証する。
pub fn verify_file_widget_caller_and_origin(
    window_label: &str,
    origin_header: Option<&str>,
    widgets: &[WidgetConfig],
) -> Result<String, String> {
    let widget_id = verify_file_widget_caller(window_label, widgets)?;
    verify_widgetfile_origin(origin_header)?;
    Ok(widget_id)
}

fn app_config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .config_dir()
        .map_err(|e| format!("failed to resolve config_dir: {e}"))?
        .join("DesktopDashboards");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create config dir {}: {e}", dir.display()))?;
    Ok(dir)
}

fn widget_storage_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_config_dir(app)?.join("storage"))
}

fn connections_file_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_config_dir(app)?.join("connections.json"))
}

fn widget_storage_file_path(storage_dir: &Path, widget_id: &str) -> Result<PathBuf, String> {
    validate_widget_id(widget_id)?;
    Ok(storage_dir.join(format!("{widget_id}.json")))
}

fn read_storage_map(path: &Path) -> Result<Map<String, Value>, String> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let text = fs::read_to_string(path)
        .map_err(|e| format!("failed to read widget storage {}: {e}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    serde_json::from_str::<Map<String, Value>>(&text)
        .map_err(|e| format!("failed to parse widget storage {}: {e}", path.display()))
}

/// 指定ガジェット専用のストレージ JSON からキーの値を読み込む（未存在時は `Value::Null`）。
pub fn read_widget_storage_value(
    storage_dir: &Path,
    widget_id: &str,
    key: &str,
) -> Result<Value, String> {
    let path = widget_storage_file_path(storage_dir, widget_id)?;
    let _guard = storage_lock()
        .lock()
        .map_err(|e| format!("storage lock poisoned: {e}"))?;
    let map = read_storage_map(&path)?;
    Ok(map.get(key).cloned().unwrap_or(Value::Null))
}

/// 指定ガジェット専用のストレージ JSON へキーの値を原子的に書き込む（`Null` で削除）。
pub fn write_widget_storage_value(
    storage_dir: &Path,
    widget_id: &str,
    key: &str,
    value: Value,
) -> Result<(), String> {
    let path = widget_storage_file_path(storage_dir, widget_id)?;
    let _guard = storage_lock()
        .lock()
        .map_err(|e| format!("storage lock poisoned: {e}"))?;

    fs::create_dir_all(storage_dir)
        .map_err(|e| format!("failed to create storage dir {}: {e}", storage_dir.display()))?;

    let mut map = read_storage_map(&path)?;
    if value.is_null() {
        map.remove(key);
    } else {
        map.insert(key.to_string(), value);
    }

    let json = serde_json::to_string_pretty(&map)
        .map_err(|e| format!("failed to serialize widget storage: {e}"))?;
    let tmp_path = storage_dir.join(format!("{widget_id}.json.tmp"));

    {
        let mut file = File::create(&tmp_path)
            .map_err(|e| format!("failed to create {}: {e}", tmp_path.display()))?;
        file.write_all(json.as_bytes())
            .map_err(|e| format!("failed to write {}: {e}", tmp_path.display()))?;
        file.sync_all()
            .map_err(|e| format!("failed to sync {}: {e}", tmp_path.display()))?;
    }

    if fs::rename(&tmp_path, &path).is_err() {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("failed to remove old {}: {e}", path.display()))?;
        }
        fs::rename(&tmp_path, &path).map_err(|e| {
            format!(
                "failed to rename {} -> {}: {e}",
                tmp_path.display(),
                path.display()
            )
        })?;
    }

    Ok(())
}

pub fn default_connections_template() -> ConnectionsFile {
    let mut headers = BTreeMap::new();
    headers.insert(
        "Notion-Version".to_string(),
        DEFAULT_NOTION_VERSION.to_string(),
    );

    let mut connections = BTreeMap::new();
    connections.insert(
        "notion".to_string(),
        ConnectionEntry {
            base_url: DEFAULT_NOTION_BASE_URL.to_string(),
            token: DEFAULT_NOTION_DUMMY_TOKEN.to_string(),
            headers,
        },
    );

    ConnectionsFile {
        version: 1,
        connections,
    }
}

/// `connections.json` が未作成の場合のみダミーテンプレートを生成する（既存ファイルは絶対に上書きしない）。
pub fn ensure_connections_template(connections_path: &Path) -> Result<(), String> {
    if connections_path.exists() {
        return Ok(());
    }
    if let Some(parent) = connections_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create dir {}: {e}", parent.display()))?;
    }

    let template = default_connections_template();
    let json = serde_json::to_string_pretty(&template)
        .map_err(|e| format!("failed to serialize connections template: {e}"))?;

    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(connections_path)
    {
        Ok(mut file) => {
            file.write_all(json.as_bytes())
                .and_then(|_| file.write_all(b"\n"))
                .and_then(|_| file.sync_all())
                .map_err(|e| {
                    format!(
                        "failed to write connections template {}: {e}",
                        connections_path.display()
                    )
                })?;
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!(
            "failed to create connections template {}: {e}",
            connections_path.display()
        )),
    }
}

/// アプリ設定ディレクトリ直下の `connections.json` テンプレートを保証する。
pub fn ensure_connections_template_for_app(app: &AppHandle) -> Result<PathBuf, String> {
    let path = connections_file_path(app)?;
    ensure_connections_template(&path)?;
    Ok(path)
}

fn is_dummy_or_empty_token(token: &str) -> bool {
    let trimmed = token.trim();
    trimmed.is_empty() || trimmed.starts_with("ntn_DUMMY") || trimmed.starts_with("secret_DUMMY")
}

/// `connections.json` から接続設定を読み込み、ダミートークンや不正 URL を検証する。
pub fn load_connection_entry(
    connections_path: &Path,
    connection: &str,
) -> Result<ConnectionEntry, String> {
    ensure_connections_template(connections_path)?;

    let text = fs::read_to_string(connections_path)
        .map_err(|e| format!("failed to read {}: {e}", connections_path.display()))?;
    let parsed: ConnectionsFile = serde_json::from_str(&text)
        .map_err(|e| format!("failed to parse {}: {e}", connections_path.display()))?;

    let mut entry = parsed
        .connections
        .get(connection)
        .cloned()
        .ok_or_else(|| format!("connection '{connection}' not found in connections.json"))?;

    entry.token = entry.token.trim().to_string();
    if is_dummy_or_empty_token(&entry.token) {
        if connection == "notion" {
            return Err(DUMMY_NOTION_TOKEN_ERR.to_string());
        }
        return Err(format!(
            "connections.json の {connection}.token がダミー値または未設定です。%APPDATA%\\DesktopDashboards\\connections.json にトークンを設定してください"
        ));
    }

    let base_trimmed = entry.base_url.trim().trim_end_matches('/').to_string();
    let parsed_url = url::Url::parse(&base_trimmed)
        .map_err(|e| format!("invalid base_url for connection '{connection}': {e}"))?;
    if !matches!(parsed_url.scheme(), "http" | "https") {
        return Err(format!(
            "unsupported base_url scheme for connection '{connection}'"
        ));
    }
    entry.base_url = base_trimmed;

    Ok(entry)
}

fn is_single_segment(segment: &str) -> bool {
    !segment.is_empty() && !segment.contains('/')
}

/// Phase 4a スパイク用に読み取り専用の 3 エンドポイントのみを許可する。
pub fn verify_phase4a_endpoint(connection: &str, method: &str, path: &str) -> Result<(), String> {
    let method_upper = method.trim().to_ascii_uppercase();

    if connection != "notion" {
        return Err(format!(
            "endpoint not allowed in Phase 4a: {method_upper} {path}"
        ));
    }

    let path_lower = path.to_ascii_lowercase();
    let has_unsafe_path = !path.starts_with('/')
        || path.contains("..")
        || path.contains('?')
        || path.contains('#')
        || path.contains("//")
        || path.contains('\\')
        || path.contains('\0')
        || path.chars().any(|c| c.is_control() || c.is_whitespace())
        || path_lower.contains("%2e")
        || path_lower.contains("%2f")
        || path_lower.contains("%5c")
        || path_lower.contains("%3f")
        || path_lower.contains("%23");

    if has_unsafe_path {
        return Err(format!(
            "endpoint not allowed in Phase 4a: {method_upper} {path}"
        ));
    }

    let allowed = match method_upper.as_str() {
        "GET" => {
            path == "/v1/users/me"
                || path
                    .strip_prefix("/v1/databases/")
                    .is_some_and(is_single_segment)
        }
        "POST" => path
            .strip_prefix("/v1/data_sources/")
            .and_then(|rest| rest.strip_suffix("/query"))
            .is_some_and(is_single_segment),
        _ => false,
    };

    if allowed {
        Ok(())
    } else {
        Err(format!(
            "endpoint not allowed in Phase 4a: {method_upper} {path}"
        ))
    }
}

fn caller_widget_id(
    app: &AppHandle,
    window: &WebviewWindow,
    ipc_req: &tauri::ipc::Request<'_>,
) -> Result<String, String> {
    let origin_header = ipc_req
        .headers()
        .get("origin")
        .and_then(|v| v.to_str().ok());
    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("config lock poisoned: {e}"))?;
    verify_file_widget_caller_and_origin(window.label(), origin_header, &cfg.widgets)
}

#[tauri::command]
pub fn dd_storage_get(
    app: AppHandle,
    window: WebviewWindow,
    ipc_req: tauri::ipc::Request<'_>,
    key: String,
) -> Result<Value, String> {
    let widget_id = caller_widget_id(&app, &window, &ipc_req)?;
    let storage_dir = widget_storage_dir(&app)?;
    read_widget_storage_value(&storage_dir, &widget_id, &key)
}

#[tauri::command]
pub fn dd_storage_set(
    app: AppHandle,
    window: WebviewWindow,
    ipc_req: tauri::ipc::Request<'_>,
    key: String,
    value: Value,
) -> Result<(), String> {
    let widget_id = caller_widget_id(&app, &window, &ipc_req)?;
    let storage_dir = widget_storage_dir(&app)?;
    write_widget_storage_value(&storage_dir, &widget_id, &key, value)
}

#[tauri::command]
pub async fn dd_connect_request(
    app: AppHandle,
    window: WebviewWindow,
    ipc_req: tauri::ipc::Request<'_>,
    request: ConnectRequest,
) -> Result<ConnectResponse, String> {
    let _widget_id = caller_widget_id(&app, &window, &ipc_req)?;
    verify_phase4a_endpoint(&request.connection, &request.method, &request.path)?;

    let conn_path = connections_file_path(&app)?;
    let entry = load_connection_entry(&conn_path, &request.connection)?;

    let method_upper = request.method.trim().to_ascii_uppercase();
    let method = reqwest::Method::from_bytes(method_upper.as_bytes())
        .map_err(|e| format!("invalid HTTP method: {e}"))?;

    let target_url = format!("{}{}", entry.base_url, request.path);

    let mut headers = HeaderMap::new();
    let auth_value = HeaderValue::from_str(&format!("Bearer {}", entry.token))
        .map_err(|_| "invalid token format in connections.json".to_string())?;
    headers.insert(AUTHORIZATION, auth_value);

    for (name, val) in &entry.headers {
        if name.eq_ignore_ascii_case("authorization") {
            continue;
        }
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| format!("invalid header name '{name}' in connections.json: {e}"))?;
        let header_val = HeaderValue::from_str(val)
            .map_err(|_| format!("invalid header value for '{name}' in connections.json"))?;
        headers.insert(header_name, header_val);
    }

    let mut req_builder = http_client().request(method, &target_url).headers(headers);
    if let Some(body) = &request.body {
        if !body.is_null() {
            req_builder = req_builder.json(body);
        }
    }

    let response = req_builder
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e.without_url()))?;
    let status = response.status().as_u16();
    let ok = (200..300).contains(&status);
    let body_text = response
        .text()
        .await
        .map_err(|e| format!("failed to read HTTP response body: {e}"))?;

    let data = if body_text.trim().is_empty() {
        Value::Null
    } else {
        match serde_json::from_str::<Value>(&body_text) {
            Ok(val) => val,
            Err(_) => serde_json::json!({ "raw_text": body_text }),
        }
    };

    Ok(ConnectResponse { status, ok, data })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_widgets() -> Vec<WidgetConfig> {
        vec![
            WidgetConfig {
                id: "file-widget-1".to_string(),
                source_type: SourceType::File,
                source: "C:\\widgets\\notion.html".to_string(),
                ..Default::default()
            },
            WidgetConfig {
                id: "url-widget-1".to_string(),
                source_type: SourceType::Url,
                source: "https://example.com".to_string(),
                ..Default::default()
            },
        ]
    }

    #[test]
    fn verify_caller_allows_file_widget() {
        let widgets = sample_widgets();
        let res = verify_file_widget_caller("widget-file-widget-1", &widgets);
        assert_eq!(res.unwrap(), "file-widget-1");
    }

    #[test]
    fn verify_caller_rejects_url_widget() {
        let widgets = sample_widgets();
        let err = verify_file_widget_caller("widget-url-widget-1", &widgets).unwrap_err();
        assert_eq!(
            err,
            "DD bridge is forbidden for URL widgets (SourceType::File required)"
        );
    }

    #[test]
    fn verify_caller_rejects_manager_and_invalid_windows() {
        let widgets = sample_widgets();
        assert!(verify_file_widget_caller("manager", &widgets).is_err());
        assert!(verify_file_widget_caller("", &widgets).is_err());
        assert!(verify_file_widget_caller("widget-", &widgets).is_err());
        assert!(verify_file_widget_caller("widget-missing-id", &widgets).is_err());
        assert!(verify_file_widget_caller("widget-../secret", &widgets).is_err());
    }

    #[test]
    fn verify_widgetfile_origin_and_file_widget_rejects_external_iframes() {
        let widgets = sample_widgets();

        // 許可: File ウィジェット + http://widgetfile.localhost
        assert_eq!(
            verify_file_widget_caller_and_origin(
                "widget-file-widget-1",
                Some("http://widgetfile.localhost"),
                &widgets,
            )
            .unwrap(),
            "file-widget-1"
        );
        assert!(verify_widgetfile_origin(Some("http://widgetfile.localhost")).is_ok());

        // 拒否: File ウィジェットウィンドウであっても外部 iframe オリジンや null / ヘッダ欠落は拒否
        for bad_origin in [
            None,
            Some(""),
            Some("null"),
            Some("https://example.com"),
            Some("http://example.com"),
            Some("https://widgetfile.localhost"),
            Some("http://widgetfile.localhost:8080"),
            Some("http://sub.widgetfile.localhost"),
            Some("http://widgetfile.localhost/subpath"),
            Some("http://widgetfile.localhost?foo=bar"),
        ] {
            let err = verify_file_widget_caller_and_origin(
                "widget-file-widget-1",
                bad_origin,
                &widgets,
            )
            .unwrap_err();
            assert!(
                err.contains("non-widgetfile origin"),
                "expected origin rejection for {bad_origin:?}, got: {err}"
            );
        }

        // 拒否: URL ウィジェットは widgetfile.localhost を偽装しても SourceType::File チェックで拒否
        let url_err = verify_file_widget_caller_and_origin(
            "widget-url-widget-1",
            Some("http://widgetfile.localhost"),
            &widgets,
        )
        .unwrap_err();
        assert_eq!(
            url_err,
            "DD bridge is forbidden for URL widgets (SourceType::File required)"
        );
    }

    #[test]
    fn phase4a_endpoint_allows_only_three_read_patterns() {
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/users/me").is_ok());
        assert!(verify_phase4a_endpoint("notion", "get", "/v1/users/me").is_ok());
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/databases/abc-123").is_ok());
        assert!(
            verify_phase4a_endpoint("notion", "POST", "/v1/data_sources/ds-456/query").is_ok()
        );
        assert!(
            verify_phase4a_endpoint("notion", "post", "/v1/data_sources/ds-456/query").is_ok()
        );
    }

    #[test]
    fn phase4a_endpoint_rejects_mutations_traversal_and_other_paths() {
        // 書き込み・他メソッド
        let patch_err = verify_phase4a_endpoint("notion", "PATCH", "/v1/pages/123").unwrap_err();
        assert!(patch_err.contains("endpoint not allowed in Phase 4a"));
        assert!(verify_phase4a_endpoint("notion", "POST", "/v1/pages").is_err());
        assert!(verify_phase4a_endpoint("notion", "DELETE", "/v1/blocks/123").is_err());
        assert!(verify_phase4a_endpoint("notion", "POST", "/v1/users/me").is_err());
        assert!(verify_phase4a_endpoint("notion", "POST", "/v1/databases/123").is_err());
        assert!(
            verify_phase4a_endpoint("notion", "GET", "/v1/data_sources/ds-123/query").is_err()
        );

        // パストラバーサル・クエリ・フラグメント・不正セグメント
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/databases/../users/me").is_err());
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/databases/db1?foo=bar").is_err());
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/databases/db1#hash").is_err());
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/databases//db1").is_err());
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/databases/db1\\x").is_err());
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/databases/%2e%2e").is_err());
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/databases/").is_err());
        assert!(verify_phase4a_endpoint("notion", "GET", "/v1/databases/a/b").is_err());
        assert!(verify_phase4a_endpoint("notion", "POST", "/v1/data_sources//query").is_err());
        assert!(verify_phase4a_endpoint("notion", "POST", "/v1/data_sources/a/b/query").is_err());
        assert!(verify_phase4a_endpoint("notion", "GET", "v1/users/me").is_err());

        // 未許可コネクション
        assert!(verify_phase4a_endpoint("other", "GET", "/v1/users/me").is_err());
    }

    #[test]
    fn widget_storage_isolates_by_widget_id_and_deletes_on_null() {
        let dir = std::env::temp_dir().join(format!("dd_storage_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        // 未作成時は Null
        assert_eq!(
            read_widget_storage_value(&dir, "w1", "notion:database_id").unwrap(),
            Value::Null
        );

        // w1 と w2 で分離して保存される
        write_widget_storage_value(&dir, "w1", "notion:database_id", json!("db-111")).unwrap();
        write_widget_storage_value(&dir, "w2", "notion:database_id", json!("db-222")).unwrap();

        assert_eq!(
            read_widget_storage_value(&dir, "w1", "notion:database_id").unwrap(),
            json!("db-111")
        );
        assert_eq!(
            read_widget_storage_value(&dir, "w2", "notion:database_id").unwrap(),
            json!("db-222")
        );
        assert!(dir.join("w1.json").is_file());
        assert!(dir.join("w2.json").is_file());

        // 複数キー保持と Null 指定によるキー削除
        write_widget_storage_value(&dir, "w1", "notion:data_source_id:db-111", json!("ds-aaa"))
            .unwrap();
        assert_eq!(
            read_widget_storage_value(&dir, "w1", "notion:database_id").unwrap(),
            json!("db-111")
        );
        assert_eq!(
            read_widget_storage_value(&dir, "w1", "notion:data_source_id:db-111").unwrap(),
            json!("ds-aaa")
        );

        write_widget_storage_value(&dir, "w1", "notion:data_source_id:db-111", Value::Null)
            .unwrap();
        assert_eq!(
            read_widget_storage_value(&dir, "w1", "notion:data_source_id:db-111").unwrap(),
            Value::Null
        );
        assert_eq!(
            read_widget_storage_value(&dir, "w1", "notion:database_id").unwrap(),
            json!("db-111")
        );

        // パストラバーサルを含む widget_id は拒否
        assert!(read_widget_storage_value(&dir, "../evil", "k").is_err());
        assert!(write_widget_storage_value(&dir, "a/b", "k", json!(1)).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn connections_template_is_created_without_overwriting_existing() {
        let dir = std::env::temp_dir().join(format!("dd_conn_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let conn_path = dir.join("connections.json");

        ensure_connections_template(&conn_path).unwrap();
        assert!(conn_path.is_file());

        let text = fs::read_to_string(&conn_path).unwrap();
        let parsed: ConnectionsFile = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.version, 1);
        let notion = parsed.connections.get("notion").unwrap();
        assert_eq!(notion.base_url, "https://api.notion.com");
        assert_eq!(notion.token, "ntn_DUMMY_REPLACE_WITH_YOUR_NOTION_TOKEN");
        assert_eq!(
            notion.headers.get("Notion-Version").map(String::as_str),
            Some("2025-09-03")
        );

        // 既存ファイルは絶対に上書きしない
        let custom_content = r#"{
  "version": 1,
  "connections": {
    "notion": {
      "base_url": "https://api.notion.com/",
      "token": "ntn_real_token_value_123",
      "headers": {
        "Notion-Version": "2025-09-03"
      }
    }
  }
}"#;
        fs::write(&conn_path, custom_content).unwrap();
        ensure_connections_template(&conn_path).unwrap();
        let after = fs::read_to_string(&conn_path).unwrap();
        assert_eq!(after, custom_content);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_connection_rejects_dummy_or_empty_tokens_and_redacts_debug() {
        let dir = std::env::temp_dir().join(format!("dd_token_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let conn_path = dir.join("connections.json");

        // テンプレートのダミートークンは拒否され、トークン値を漏らさない
        ensure_connections_template(&conn_path).unwrap();
        let dummy_err = load_connection_entry(&conn_path, "notion").unwrap_err();
        assert!(dummy_err.contains("connections.json"));
        assert!(!dummy_err.contains("ntn_DUMMY_REPLACE_WITH_YOUR_NOTION_TOKEN"));

        // secret_DUMMY で始まる場合も拒否
        fs::write(
            &conn_path,
            r#"{"version":1,"connections":{"notion":{"base_url":"https://api.notion.com","token":"secret_DUMMY_XYZ","headers":{}}}}"#,
        )
        .unwrap();
        let secret_dummy_err = load_connection_entry(&conn_path, "notion").unwrap_err();
        assert!(secret_dummy_err.contains("connections.json"));
        assert!(!secret_dummy_err.contains("secret_DUMMY_XYZ"));

        // 空トークンも拒否
        fs::write(
            &conn_path,
            r#"{"version":1,"connections":{"notion":{"base_url":"https://api.notion.com","token":"   ","headers":{}}}}"#,
        )
        .unwrap();
        assert!(load_connection_entry(&conn_path, "notion").is_err());

        // 有効トークンは読み込み成功、末尾スラッシュはトリム、Debug 出力にトークンを含まない
        let secret_val = "ntn_test_live_secret_987654";
        fs::write(
            &conn_path,
            format!(
                r#"{{"version":1,"connections":{{"notion":{{"base_url":"https://api.notion.com/","token":"{secret_val}","headers":{{"Notion-Version":"2025-09-03"}}}}}}}}"#
            ),
        )
        .unwrap();
        let entry = load_connection_entry(&conn_path, "notion").unwrap();
        assert_eq!(entry.base_url, "https://api.notion.com");
        assert_eq!(entry.token, secret_val);
        let debug_str = format!("{entry:?}");
        assert!(!debug_str.contains(secret_val));
        assert!(debug_str.contains("[REDACTED]"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn widgets_dd_capability_enables_local_and_excludes_remote() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let widgets_dd_path = manifest_dir.join("capabilities").join("widgets-dd.json");
        let widgets_path = manifest_dir.join("capabilities").join("widgets.json");

        let widgets_dd_text = fs::read_to_string(&widgets_dd_path).unwrap();
        let widgets_dd: Value = serde_json::from_str(&widgets_dd_text).unwrap();
        assert_eq!(widgets_dd.get("local"), Some(&json!(true)));
        assert!(
            widgets_dd.get("remote").is_none(),
            "widgets-dd.json must not define remote urls"
        );
        assert_eq!(widgets_dd.get("windows"), Some(&json!(["widget-*"])));
        let dd_perms = widgets_dd
            .get("permissions")
            .and_then(Value::as_array)
            .expect("permissions array");
        for required in [
            "allow-dd-storage-get",
            "allow-dd-storage-set",
            "allow-dd-connect-request",
        ] {
            assert!(
                dd_perms.iter().any(|p| p.as_str() == Some(required)),
                "missing permission {required} in widgets-dd.json"
            );
        }

        let widgets_text = fs::read_to_string(&widgets_path).unwrap();
        let widgets: Value = serde_json::from_str(&widgets_text).unwrap();
        let widget_perms = widgets
            .get("permissions")
            .and_then(Value::as_array)
            .expect("permissions array");
        assert!(
            !widget_perms
                .iter()
                .filter_map(Value::as_str)
                .any(|p| p.starts_with("allow-dd-")),
            "widgets.json must not contain allow-dd-* permissions"
        );
    }
}
