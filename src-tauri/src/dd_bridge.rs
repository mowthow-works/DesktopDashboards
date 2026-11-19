use crate::config::{ConfigState, SourceType, WidgetConfig};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

const FORBIDDEN_URL_WIDGET_ERR: &str =
    "DD bridge is forbidden for URL widgets (SourceType::File required)";
const BUILTIN_NOTION_CONNECTOR_JSON: &str = include_str!("../../connectors/notion.json");

static STORAGE_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static WIDGET_CONFIG_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CONNECTIONS_IO_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static HTTP_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

fn storage_lock() -> &'static Mutex<()> {
    STORAGE_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn widget_config_lock() -> &'static Mutex<()> {
    WIDGET_CONFIG_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn connections_lock() -> &'static Mutex<()> {
    CONNECTIONS_IO_LOCK.get_or_init(|| Mutex::new(()))
}

fn http_client() -> &'static reqwest::Client {
    HTTP_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    Bearer {
        credential_key: String,
    },
    ApiKeyHeader {
        header_name: String,
        credential_key: String,
        #[serde(default)]
        prefix: Option<String>,
    },
}

impl AuthConfig {
    pub fn credential_key(&self) -> &str {
        match self {
            Self::Bearer { credential_key } => credential_key,
            Self::ApiKeyHeader { credential_key, .. } => credential_key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialField {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointRule {
    pub method: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectorManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub official: bool,
    pub base_url: String,
    pub allowed_hosts: Vec<String>,
    pub auth: AuthConfig,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub credential_schema: Vec<CredentialField>,
    pub test: EndpointRule,
    #[serde(default)]
    pub endpoints: Vec<EndpointRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetConnectionGrant {
    pub connection: String,
    #[serde(default)]
    pub connector: Option<String>,
    #[serde(default)]
    pub endpoints: Vec<EndpointRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigFieldSchema {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<Value>,
    #[serde(default)]
    pub placeholder: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WidgetConfigFormDto {
    pub widget_id: String,
    pub has_manifest: bool,
    pub manifest_id: Option<String>,
    pub manifest_name: Option<String>,
    pub schema: Vec<ConfigFieldSchema>,
    pub values: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WidgetManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub entry: Option<String>,
    #[serde(default)]
    pub connector: Option<String>,
    #[serde(default)]
    pub connections: Vec<WidgetConnectionGrant>,
    #[serde(default)]
    pub endpoints: Vec<EndpointRule>,
    #[serde(default)]
    pub config_schema: Vec<ConfigFieldSchema>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionsFileV2 {
    pub version: u32,
    pub connections: BTreeMap<String, ConnectionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectionRecord {
    pub name: String,
    pub connector_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectorSummaryDto {
    pub id: String,
    pub name: String,
    pub official: bool,
    pub base_url: String,
    pub allowed_hosts: Vec<String>,
    pub credential_schema: Vec<CredentialField>,
    pub test_method: String,
    pub test_path: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConnectionSummaryDto {
    pub id: String,
    pub name: String,
    pub connector_id: String,
    pub connector_name: String,
    pub official: bool,
    pub base_url: String,
    pub allowed_hosts: Vec<String>,
    pub has_secret: bool,
    pub configured_keys: Vec<String>,
}

#[derive(Clone, Deserialize)]
pub struct SaveConnectionInput {
    pub id: String,
    pub name: String,
    pub connector_id: String,
    #[serde(default)]
    pub credentials: BTreeMap<String, String>,
}

impl fmt::Debug for SaveConnectionInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaveConnectionInput")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("connector_id", &self.connector_id)
            .field("credentials", &"[REDACTED]")
            .finish()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TestConnectionResultDto {
    pub connection_id: String,
    pub connector_id: String,
    pub ok: bool,
    pub status: u16,
    pub summary: String,
    pub detail: String,
    pub hint: Option<String>,
}

/// 資格情報ストア抽象トレイト（本番: `Win32CredentialStore` / テスト: `InMemoryCredentialStore`）。
pub trait CredentialStore: Send + Sync {
    fn get_secret(&self, target_name: &str) -> Result<Option<String>, String>;
    fn set_secret(&self, target_name: &str, secret: &str) -> Result<(), String>;
    #[allow(dead_code)]
    fn delete_secret(&self, target_name: &str) -> Result<(), String>;
}

pub fn credential_target_name(connection_id: &str, credential_key: &str) -> String {
    format!("DesktopDashboards:connection:{connection_id}:{credential_key}")
}

fn encode_credential_blob_utf16le(secret: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(secret.len() * 2);
    for unit in secret.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn decode_credential_blob(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() {
        return Ok(String::new());
    }
    if bytes.len() % 2 == 0 {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        if let Ok(mut s) = String::from_utf16(&units) {
            while s.ends_with('\0') {
                s.pop();
            }
            return Ok(s);
        }
    }
    String::from_utf8(bytes.to_vec())
        .map(|mut s| {
            while s.ends_with('\0') {
                s.pop();
            }
            s
        })
        .map_err(|_| "failed to decode stored credential bytes".to_string())
}

#[allow(dead_code)]
#[derive(Default)]
pub struct InMemoryCredentialStore {
    entries: Mutex<BTreeMap<String, String>>,
}

impl fmt::Debug for InMemoryCredentialStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = self.entries.lock().map(|m| m.len()).unwrap_or(0);
        f.debug_struct("InMemoryCredentialStore")
            .field("entry_count", &count)
            .finish()
    }
}

impl CredentialStore for InMemoryCredentialStore {
    fn get_secret(&self, target_name: &str) -> Result<Option<String>, String> {
        let guard = self
            .entries
            .lock()
            .map_err(|e| format!("credential store lock poisoned: {e}"))?;
        Ok(guard.get(target_name).cloned())
    }

    fn set_secret(&self, target_name: &str, secret: &str) -> Result<(), String> {
        let mut guard = self
            .entries
            .lock()
            .map_err(|e| format!("credential store lock poisoned: {e}"))?;
        guard.insert(target_name.to_string(), secret.to_string());
        Ok(())
    }

    fn delete_secret(&self, target_name: &str) -> Result<(), String> {
        let mut guard = self
            .entries
            .lock()
            .map_err(|e| format!("credential store lock poisoned: {e}"))?;
        guard.remove(target_name);
        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Win32CredentialStore;

#[cfg(windows)]
impl CredentialStore for Win32CredentialStore {
    fn get_secret(&self, target_name: &str) -> Result<Option<String>, String> {
        use std::ffi::c_void;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::ERROR_NOT_FOUND;
        use windows::Win32::Security::Credentials::{
            CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
        };

        let target_wide: Vec<u16> = target_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut cred_ptr: *mut CREDENTIALW = std::ptr::null_mut();

        let res = unsafe {
            CredReadW(
                PCWSTR(target_wide.as_ptr()),
                CRED_TYPE_GENERIC,
                Some(0),
                &mut cred_ptr,
            )
        };

        match res {
            Ok(()) => {
                if cred_ptr.is_null() {
                    return Ok(None);
                }
                let decoded = unsafe {
                    let cred = &*cred_ptr;
                    let blob = if cred.CredentialBlob.is_null() || cred.CredentialBlobSize == 0 {
                        &[]
                    } else {
                        std::slice::from_raw_parts(
                            cred.CredentialBlob,
                            cred.CredentialBlobSize as usize,
                        )
                    };
                    let parsed = decode_credential_blob(blob);
                    CredFree(cred_ptr as *const c_void);
                    parsed
                }?;
                Ok(Some(decoded))
            }
            Err(err) if err.code() == ERROR_NOT_FOUND.to_hresult() => Ok(None),
            Err(err) => Err(format!(
                "failed to read credential '{target_name}' from Windows Credential Manager (code: {:?})",
                err.code()
            )),
        }
    }

    fn set_secret(&self, target_name: &str, secret: &str) -> Result<(), String> {
        use windows::core::PWSTR;
        use windows::Win32::Security::Credentials::{
            CredWriteW, CREDENTIALW, CRED_FLAGS, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
        };

        let mut target_wide: Vec<u16> = target_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut user_wide: Vec<u16> = "DesktopDashboards"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut blob_bytes = encode_credential_blob_utf16le(secret);
        if blob_bytes.len() > 2560 {
            return Err(format!(
                "credential secret for '{target_name}' exceeds Windows Credential Manager size limit"
            ));
        }

        let cred = CREDENTIALW {
            Flags: CRED_FLAGS(0),
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR(target_wide.as_mut_ptr()),
            Comment: PWSTR::null(),
            LastWritten: Default::default(),
            CredentialBlobSize: blob_bytes.len() as u32,
            CredentialBlob: if blob_bytes.is_empty() {
                std::ptr::null_mut()
            } else {
                blob_bytes.as_mut_ptr()
            },
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: PWSTR::null(),
            UserName: PWSTR(user_wide.as_mut_ptr()),
        };

        let res = unsafe { CredWriteW(&cred, 0) };
        blob_bytes.fill(0);

        res.map_err(|err| {
            format!(
                "failed to write credential '{target_name}' to Windows Credential Manager (code: {:?})",
                err.code()
            )
        })
    }

    fn delete_secret(&self, target_name: &str) -> Result<(), String> {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::ERROR_NOT_FOUND;
        use windows::Win32::Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC};

        let target_wide: Vec<u16> = target_name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        match unsafe { CredDeleteW(PCWSTR(target_wide.as_ptr()), CRED_TYPE_GENERIC, Some(0)) } {
            Ok(()) => Ok(()),
            Err(err) if err.code() == ERROR_NOT_FOUND.to_hresult() => Ok(()),
            Err(err) => Err(format!(
                "failed to delete credential '{target_name}' from Windows Credential Manager (code: {:?})",
                err.code()
            )),
        }
    }
}

#[cfg(not(windows))]
impl CredentialStore for Win32CredentialStore {
    fn get_secret(&self, _target_name: &str) -> Result<Option<String>, String> {
        Err("Windows Credential Manager is only supported on Windows".to_string())
    }

    fn set_secret(&self, _target_name: &str, _secret: &str) -> Result<(), String> {
        Err("Windows Credential Manager is only supported on Windows".to_string())
    }

    fn delete_secret(&self, _target_name: &str) -> Result<(), String> {
        Err("Windows Credential Manager is only supported on Windows".to_string())
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

/// 接続管理コマンドの呼び出し元が管理画面ウィンドウ (`"manager"`) のみであることを検証する。
pub fn verify_manager_caller(window_label: &str) -> Result<(), String> {
    if window_label == "manager" {
        Ok(())
    } else {
        Err("connection management commands are only allowed from the manager window".to_string())
    }
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

fn widget_config_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_config_dir(app)?.join("widget_config"))
}

fn connections_file_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_config_dir(app)?.join("connections.json"))
}

fn user_connectors_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_config_dir(app)?.join("connectors"))
}

fn resolve_bundled_connectors_dirs(app: &AppHandle) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(resource_dir) = app.path().resource_dir() {
        dirs.push(resource_dir.join("connectors"));
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            dirs.push(exe_dir.join("connectors"));
        }
    }
    if let Some(dev_dir) = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|d| d.join("connectors"))
    {
        dirs.push(dev_dir);
    }
    dirs
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

fn write_text_atomic(target_path: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = target_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create dir {}: {e}", parent.display()))?;
    }
    let mut tmp_name = target_path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp_path = target_path.with_file_name(tmp_name);

    {
        let mut file = File::create(&tmp_path)
            .map_err(|e| format!("failed to create {}: {e}", tmp_path.display()))?;
        file.write_all(text.as_bytes())
            .map_err(|e| format!("failed to write {}: {e}", tmp_path.display()))?;
        file.sync_all()
            .map_err(|e| format!("failed to sync {}: {e}", tmp_path.display()))?;
    }

    if fs::rename(&tmp_path, target_path).is_err() {
        if target_path.exists() {
            fs::remove_file(target_path)
                .map_err(|e| format!("failed to remove old {}: {e}", target_path.display()))?;
        }
        fs::rename(&tmp_path, target_path).map_err(|e| {
            format!(
                "failed to rename {} -> {}: {e}",
                tmp_path.display(),
                target_path.display()
            )
        })?;
    }

    Ok(())
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

    let mut json = serde_json::to_string_pretty(&map)
        .map_err(|e| format!("failed to serialize widget storage: {e}"))?;
    json.push('\n');
    write_text_atomic(&path, &json)
}

fn is_safe_identifier(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn is_valid_http_method(method: &str) -> bool {
    let upper = method.trim().to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
    )
}

/// リクエスト実パスの安全性検証（`/` 始まり・パストラバーサル/クエリ/フラグメント/二重スラッシュ/エンコード細工・絶対 URL 禁止）。
pub fn verify_request_path_safety(path: &str) -> Result<(), String> {
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
        Err(format!("unsafe request path rejected: {path}"))
    } else {
        Ok(())
    }
}

/// エンドポイント定義パターンの安全性検証（`*` は単一パスセグメントワイルドカードのみ許可）。
pub fn verify_endpoint_pattern_safety(pattern_path: &str) -> Result<(), String> {
    verify_request_path_safety(pattern_path)?;
    if pattern_path == "/" {
        return Ok(());
    }
    let rest = pattern_path
        .strip_prefix('/')
        .ok_or_else(|| format!("endpoint pattern must start with '/': {pattern_path}"))?;
    for seg in rest.split('/') {
        if seg.is_empty() {
            return Err(format!(
                "endpoint pattern contains empty segment: {pattern_path}"
            ));
        }
        if seg.contains('*') && seg != "*" {
            return Err(format!(
                "wildcard '*' must occupy an entire path segment: {pattern_path}"
            ));
        }
    }
    Ok(())
}

/// ルール `(rule_method, rule_path)` が実リクエスト `(req_method, req_path)` に一致するか判定する。
pub fn endpoint_pattern_matches(
    rule_method: &str,
    rule_path: &str,
    req_method: &str,
    req_path: &str,
) -> bool {
    if !rule_method.trim().eq_ignore_ascii_case(req_method.trim()) {
        return false;
    }
    let Some(rule_rest) = rule_path.strip_prefix('/') else {
        return false;
    };
    let Some(req_rest) = req_path.strip_prefix('/') else {
        return false;
    };
    if rule_rest.is_empty() && req_rest.is_empty() {
        return true;
    }

    let rule_segs: Vec<&str> = rule_rest.split('/').collect();
    let req_segs: Vec<&str> = req_rest.split('/').collect();
    if rule_segs.len() != req_segs.len() {
        return false;
    }

    rule_segs.iter().zip(req_segs.iter()).all(|(&pat, &req)| {
        if req.is_empty() {
            return false;
        }
        pat == "*" || pat == req
    })
}

fn validate_connector_manifest(manifest: &mut ConnectorManifest) -> Result<(), String> {
    if !is_safe_identifier(&manifest.id) {
        return Err("connector 'id' must be non-empty ASCII alphanumeric, '-' or '_'".to_string());
    }
    if manifest.name.trim().is_empty() {
        return Err("connector 'name' must not be empty".to_string());
    }
    if manifest.allowed_hosts.is_empty() {
        return Err("connector 'allowed_hosts' must not be empty".to_string());
    }
    for host in &manifest.allowed_hosts {
        if host.is_empty()
            || host.contains('*')
            || host.contains('/')
            || host.contains('\\')
            || host.contains(':')
            || host.contains('?')
            || host.contains('#')
            || host.contains('@')
            || host.chars().any(|c| c.is_whitespace() || c.is_control())
            || url::Host::parse(host).is_err()
        {
            return Err(format!("invalid allowed_hosts entry '{host}'"));
        }
    }

    if manifest.base_url != manifest.base_url.trim() || manifest.base_url.is_empty() {
        return Err("connector 'base_url' must not be empty or have surrounding whitespace".to_string());
    }
    let parsed_base = url::Url::parse(&manifest.base_url)
        .map_err(|e| format!("invalid connector base_url: {e}"))?;
    if parsed_base.scheme() != "https" {
        return Err("connector 'base_url' must use https scheme".to_string());
    }
    if !parsed_base.username().is_empty() || parsed_base.password().is_some() {
        return Err("connector 'base_url' must not contain userinfo".to_string());
    }
    if !(parsed_base.path().is_empty() || parsed_base.path() == "/") {
        return Err("connector 'base_url' must not contain a path".to_string());
    }
    if parsed_base.query().is_some() || parsed_base.fragment().is_some() {
        return Err("connector 'base_url' must not contain query or fragment".to_string());
    }
    let base_host = parsed_base
        .host_str()
        .ok_or_else(|| "connector 'base_url' is missing host".to_string())?;
    if !manifest
        .allowed_hosts
        .iter()
        .any(|h| h.eq_ignore_ascii_case(base_host))
    {
        return Err("connector 'base_url' host must be listed in 'allowed_hosts'".to_string());
    }
    manifest.base_url = manifest.base_url.trim_end_matches('/').to_string();

    match &manifest.auth {
        AuthConfig::Bearer { credential_key } => {
            if !is_safe_identifier(credential_key) {
                return Err("connector auth 'credential_key' is invalid".to_string());
            }
        }
        AuthConfig::ApiKeyHeader {
            header_name,
            credential_key,
            ..
        } => {
            if !is_safe_identifier(credential_key) {
                return Err("connector auth 'credential_key' is invalid".to_string());
            }
            if header_name.trim().is_empty()
                || header_name.eq_ignore_ascii_case("host")
                || header_name.eq_ignore_ascii_case("content-length")
                || HeaderName::from_bytes(header_name.as_bytes()).is_err()
            {
                return Err(format!("invalid auth header_name '{header_name}'"));
            }
        }
    }

    for (name, val) in &manifest.headers {
        if name.eq_ignore_ascii_case("authorization") {
            return Err("connector 'headers' must not contain 'authorization'".to_string());
        }
        if HeaderName::from_bytes(name.as_bytes()).is_err() {
            return Err(format!("invalid connector header name '{name}'"));
        }
        if HeaderValue::from_str(val).is_err() {
            return Err(format!("invalid connector header value for '{name}'"));
        }
    }

    if !is_valid_http_method(&manifest.test.method) {
        return Err(format!(
            "invalid test HTTP method '{}'",
            manifest.test.method
        ));
    }
    verify_request_path_safety(&manifest.test.path)?;
    if manifest.test.path.contains('*') {
        return Err("connector 'test.path' must be a fixed path without '*'".to_string());
    }

    for rule in &manifest.endpoints {
        if !is_valid_http_method(&rule.method) {
            return Err(format!("invalid endpoint HTTP method '{}'", rule.method));
        }
        verify_endpoint_pattern_safety(&rule.path)?;
    }

    Ok(())
}

/// コネクタ定義 JSON 文字列をパース・スキーマ検証する。
pub fn parse_and_validate_connector(json_text: &str) -> Result<ConnectorManifest, String> {
    let mut manifest: ConnectorManifest =
        serde_json::from_str(json_text).map_err(|e| format!("JSON parse error: {e}"))?;
    validate_connector_manifest(&mut manifest)?;
    Ok(manifest)
}

fn collect_sorted_json_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|res| res.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    files.sort();
    files
}

/// ビルトイン同梱・バンドルフォルダ・ユーザー配置フォルダからコネクタ定義を読み込む。
pub fn load_connectors_from_dirs(
    bundled_dirs: &[PathBuf],
    user_dir: Option<&Path>,
) -> BTreeMap<String, ConnectorManifest> {
    let mut registry = BTreeMap::new();
    let mut builtin_official_ids = HashSet::new();

    match parse_and_validate_connector(BUILTIN_NOTION_CONNECTOR_JSON) {
        Ok(mut notion) => {
            notion.official = true;
            builtin_official_ids.insert(notion.id.clone());
            registry.insert(notion.id.clone(), notion);
        }
        Err(reason) => {
            eprintln!("[connector] failed to parse built-in notion connector: {reason}");
        }
    }

    for dir in bundled_dirs {
        if !dir.is_dir() {
            continue;
        }
        for file in collect_sorted_json_files(dir) {
            let text = match fs::read_to_string(&file) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!(
                        "[connector] ignored unreadable bundled connector {}: {e}",
                        file.display()
                    );
                    continue;
                }
            };
            match parse_and_validate_connector(&text) {
                Ok(manifest) => {
                    if builtin_official_ids.contains(&manifest.id)
                        || registry.contains_key(&manifest.id)
                    {
                        continue;
                    }
                    if manifest.official {
                        builtin_official_ids.insert(manifest.id.clone());
                    }
                    registry.insert(manifest.id.clone(), manifest);
                }
                Err(reason) => {
                    eprintln!(
                        "[connector] ignored invalid connector manifest {}: {reason}",
                        file.display()
                    );
                }
            }
        }
    }

    if let Some(dir) = user_dir {
        if dir.is_dir() {
            for file in collect_sorted_json_files(dir) {
                let text = match fs::read_to_string(&file) {
                    Ok(t) => t,
                    Err(e) => {
                        eprintln!(
                            "[connector] ignored unreadable user connector {}: {e}",
                            file.display()
                        );
                        continue;
                    }
                };
                match parse_and_validate_connector(&text) {
                    Ok(mut manifest) => {
                        manifest.official = false;
                        if builtin_official_ids.contains(&manifest.id) {
                            eprintln!(
                                "[connector] ignored invalid connector manifest {}: cannot override built-in official connector '{}'",
                                file.display(),
                                manifest.id
                            );
                            continue;
                        }
                        registry.insert(manifest.id.clone(), manifest);
                    }
                    Err(reason) => {
                        eprintln!(
                            "[connector] ignored invalid connector manifest {}: {reason}",
                            file.display()
                        );
                    }
                }
            }
        }
    }

    registry
}

pub fn load_connectors_for_app(app: &AppHandle) -> BTreeMap<String, ConnectorManifest> {
    let bundled_dirs = resolve_bundled_connectors_dirs(app);
    let user_dir = user_connectors_dir(app).ok();
    load_connectors_from_dirs(&bundled_dirs, user_dir.as_deref())
}

fn validate_widget_manifest(manifest: &WidgetManifest) -> Result<(), String> {
    if manifest.id.trim().is_empty() {
        return Err("invalid widget manifest: 'id' must not be empty".to_string());
    }
    if manifest.name.trim().is_empty() {
        return Err("invalid widget manifest: 'name' must not be empty".to_string());
    }
    if let Some(connector) = manifest.connector.as_deref() {
        if !is_safe_identifier(connector) {
            return Err("invalid widget manifest: invalid 'connector'".to_string());
        }
    }
    for grant in &manifest.connections {
        if grant.connection.trim().is_empty() {
            return Err("invalid widget manifest: connection name must not be empty".to_string());
        }
        for rule in &grant.endpoints {
            if !is_valid_http_method(&rule.method) {
                return Err(format!(
                    "invalid widget manifest: invalid endpoint method '{}'",
                    rule.method
                ));
            }
            verify_endpoint_pattern_safety(&rule.path)
                .map_err(|e| format!("invalid widget manifest: {e}"))?;
        }
    }
    for rule in &manifest.endpoints {
        if !is_valid_http_method(&rule.method) {
            return Err(format!(
                "invalid widget manifest: invalid endpoint method '{}'",
                rule.method
            ));
        }
        verify_endpoint_pattern_safety(&rule.path)
            .map_err(|e| format!("invalid widget manifest: {e}"))?;
    }

    let mut seen_config_keys = HashSet::new();
    for field in &manifest.config_schema {
        if !is_safe_identifier(&field.key) {
            return Err(format!(
                "invalid widget manifest: invalid config_schema key '{}'",
                field.key
            ));
        }
        if !seen_config_keys.insert(field.key.clone()) {
            return Err(format!(
                "invalid widget manifest: duplicate config_schema key '{}'",
                field.key
            ));
        }
        if field.label.trim().is_empty() {
            return Err(format!(
                "invalid widget manifest: config_schema '{}' label must not be empty",
                field.key
            ));
        }
        match field.field_type.as_str() {
            "string" | "boolean" => {}
            other => {
                return Err(format!(
                    "invalid widget manifest: unsupported config_schema type '{other}' for '{}' (only 'string' or 'boolean' allowed)",
                    field.key
                ));
            }
        }
        if let Some(default_val) = &field.default {
            if !default_val.is_null() {
                let type_ok = match field.field_type.as_str() {
                    "string" => default_val.is_string(),
                    "boolean" => default_val.is_boolean(),
                    _ => false,
                };
                if !type_ok {
                    return Err(format!(
                        "invalid widget manifest: config_schema '{}' default value does not match type '{}'",
                        field.key, field.field_type
                    ));
                }
            }
        }
    }

    Ok(())
}

/// ウィジェット manifest JSON をパース・検証する。
pub fn parse_and_validate_widget_manifest(json_text: &str) -> Result<WidgetManifest, String> {
    let manifest: WidgetManifest =
        serde_json::from_str(json_text).map_err(|e| format!("invalid widget manifest: {e}"))?;
    validate_widget_manifest(&manifest)?;
    Ok(manifest)
}

/// HTML パスに対応するウィジェット manifest を優先順（`<stem>.manifest.json` → `manifest.json`）で読み込む。
pub fn load_widget_manifest_for_html(html_path: &Path) -> Result<Option<WidgetManifest>, String> {
    let Some(parent_dir) = html_path.parent() else {
        return Ok(None);
    };
    let html_file_name = html_path.file_name().and_then(|s| s.to_str());
    let html_stem = html_path.file_stem().and_then(|s| s.to_str());

    if let Some(stem) = html_stem {
        let stem_manifest_path = parent_dir.join(format!("{stem}.manifest.json"));
        if stem_manifest_path.is_file() {
            let text = fs::read_to_string(&stem_manifest_path)
                .map_err(|e| format!("invalid widget manifest: failed to read: {e}"))?;
            let manifest = parse_and_validate_widget_manifest(&text)?;
            if let (Some(entry), Some(fname)) = (manifest.entry.as_deref(), html_file_name) {
                if entry != fname {
                    return Err(format!(
                        "invalid widget manifest: entry '{entry}' does not match '{fname}'"
                    ));
                }
            }
            return Ok(Some(manifest));
        }
    }

    let shared_manifest_path = parent_dir.join("manifest.json");
    if shared_manifest_path.is_file() {
        let text = fs::read_to_string(&shared_manifest_path)
            .map_err(|e| format!("invalid widget manifest: failed to read manifest.json: {e}"))?;
        let manifest = parse_and_validate_widget_manifest(&text)?;
        if let Some(entry) = manifest.entry.as_deref() {
            if Some(entry) != html_file_name {
                return Ok(None);
            }
        }
        return Ok(Some(manifest));
    }

    Ok(None)
}

/// コネクタ定義とウィジェット manifest の権限積集合および送信先 HTTPS/allowed_hosts を検証する。
pub fn verify_endpoint_intersection(
    connector: &ConnectorManifest,
    widget_manifest: &WidgetManifest,
    connection: &str,
    method: &str,
    path: &str,
) -> Result<(), String> {
    verify_request_path_safety(path)?;
    if !is_valid_http_method(method) {
        return Err(format!("invalid HTTP method: {method}"));
    }
    let method_upper = method.trim().to_ascii_uppercase();

    let exact_grants: Vec<&WidgetConnectionGrant> = widget_manifest
        .connections
        .iter()
        .filter(|g| g.connection == connection)
        .collect();

    let matching_grants: Vec<&WidgetConnectionGrant> = if !exact_grants.is_empty() {
        exact_grants
    } else {
        widget_manifest
            .connections
            .iter()
            .filter(|g| {
                g.connection == "*" && g.connector.as_deref() == Some(connector.id.as_str())
            })
            .collect()
    };

    let widget_rules: Vec<&EndpointRule> = if matching_grants.is_empty() {
        if widget_manifest.connections.is_empty() && !widget_manifest.endpoints.is_empty() {
            if let Some(expected_connector) = widget_manifest.connector.as_deref() {
                if expected_connector != connector.id {
                    return Err(format!(
                        "connector mismatch for connection '{connection}': expected '{expected_connector}', got '{}'",
                        connector.id
                    ));
                }
            }
            widget_manifest.endpoints.iter().collect()
        } else {
            return Err(format!(
                "connection '{connection}' is not declared in widget manifest"
            ));
        }
    } else {
        for grant in &matching_grants {
            let expected_opt = grant
                .connector
                .as_deref()
                .or(widget_manifest.connector.as_deref());
            if let Some(expected_connector) = expected_opt {
                if expected_connector != connector.id {
                    return Err(format!(
                        "connector mismatch for connection '{connection}': expected '{expected_connector}', got '{}'",
                        connector.id
                    ));
                }
            }
        }
        let mut rules: Vec<&EndpointRule> = matching_grants
            .iter()
            .flat_map(|g| g.endpoints.iter())
            .collect();
        if rules.is_empty() && !widget_manifest.endpoints.is_empty() {
            rules = widget_manifest.endpoints.iter().collect();
        }
        rules
    };

    let allowed_by_widget = widget_rules
        .iter()
        .any(|rule| endpoint_pattern_matches(&rule.method, &rule.path, &method_upper, path));
    if !allowed_by_widget {
        return Err(format!(
            "endpoint not allowed by widget manifest: {method_upper} {path}"
        ));
    }

    let allowed_by_connector =
        endpoint_pattern_matches(&connector.test.method, &connector.test.path, &method_upper, path)
            || connector
                .endpoints
                .iter()
                .any(|rule| endpoint_pattern_matches(&rule.method, &rule.path, &method_upper, path));
    if !allowed_by_connector {
        return Err(format!(
            "endpoint not allowed by connector '{}': {method_upper} {path}",
            connector.id
        ));
    }

    let target_url = format!("{}{}", connector.base_url.trim_end_matches('/'), path);
    let parsed_target =
        url::Url::parse(&target_url).map_err(|e| format!("invalid target URL: {e}"))?;
    if parsed_target.scheme() != "https" {
        return Err("target URL must use https scheme".to_string());
    }
    let Some(host) = parsed_target.host_str() else {
        return Err("target URL is missing host".to_string());
    };
    if !connector
        .allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
    {
        return Err(format!(
            "target host '{host}' is not allowed for connector '{}'",
            connector.id
        ));
    }

    Ok(())
}

pub fn default_connections_v2() -> ConnectionsFileV2 {
    let mut connections = BTreeMap::new();
    connections.insert(
        "notion".to_string(),
        ConnectionRecord {
            name: "Notion".to_string(),
            connector_id: "notion".to_string(),
        },
    );
    ConnectionsFileV2 {
        version: 2,
        connections,
    }
}

fn is_dummy_or_empty_token(token: &str) -> bool {
    let trimmed = token.trim();
    trimmed.is_empty() || trimmed.starts_with("ntn_DUMMY") || trimmed.starts_with("secret_DUMMY")
}

fn is_clean_v2_connections_value(val: &Value) -> bool {
    let Some(obj) = val.as_object() else {
        return false;
    };
    if obj.get("version").and_then(Value::as_u64) != Some(2) {
        return false;
    }
    let Some(conns) = obj.get("connections").and_then(Value::as_object) else {
        return false;
    };
    conns.values().all(|entry| {
        let Some(entry_obj) = entry.as_object() else {
            return false;
        };
        entry_obj.get("name").and_then(Value::as_str).is_some()
            && entry_obj
                .get("connector_id")
                .and_then(Value::as_str)
                .is_some()
            && !entry_obj.contains_key("token")
            && !entry_obj.contains_key("base_url")
            && !entry_obj.contains_key("headers")
    })
}

fn migrate_and_ensure_connections_unlocked(
    connections_path: &Path,
    cred_store: &dyn CredentialStore,
) -> Result<ConnectionsFileV2, String> {
    if !connections_path.exists() {
        if let Some(parent) = connections_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create dir {}: {e}", parent.display()))?;
        }
        let template = default_connections_v2();
        let mut json = serde_json::to_string_pretty(&template)
            .map_err(|e| format!("failed to serialize connections v2 template: {e}"))?;
        json.push('\n');

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(connections_path)
        {
            Ok(mut file) => {
                file.write_all(json.as_bytes())
                    .and_then(|_| file.sync_all())
                    .map_err(|e| {
                        format!(
                            "failed to write connections.json template {}: {e}",
                            connections_path.display()
                        )
                    })?;
                return Ok(template);
            }
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => {
                return Err(format!(
                    "failed to create connections.json template {}: {e}",
                    connections_path.display()
                ))
            }
        }
    }

    let text = fs::read_to_string(connections_path)
        .map_err(|e| format!("failed to read {}: {e}", connections_path.display()))?;
    let parsed_val: Value = serde_json::from_str(&text)
        .map_err(|e| format!("failed to parse {}: {e}", connections_path.display()))?;

    if is_clean_v2_connections_value(&parsed_val) {
        return serde_json::from_value(parsed_val)
            .map_err(|e| format!("failed to deserialize v2 connections.json: {e}"));
    }

    let mut migrated_map = BTreeMap::new();
    if let Some(conns_obj) = parsed_val.get("connections").and_then(Value::as_object) {
        for (conn_id, entry_val) in conns_obj {
            let entry_obj = entry_val.as_object();
            if let Some(raw_token) = entry_obj
                .and_then(|o| o.get("token"))
                .and_then(Value::as_str)
            {
                let trimmed = raw_token.trim();
                if !is_dummy_or_empty_token(trimmed) {
                    let target = credential_target_name(conn_id, "token");
                    cred_store.set_secret(&target, trimmed)?;
                    eprintln!(
                        "[dd_bridge] migrated credential for connection '{conn_id}' to Windows Credential Manager"
                    );
                }
            }

            let connector_id = entry_obj
                .and_then(|o| o.get("connector_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(conn_id.as_str())
                .to_string();
            let name = entry_obj
                .and_then(|o| o.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or(if conn_id == "notion" {
                    "Notion"
                } else {
                    conn_id.as_str()
                })
                .to_string();

            migrated_map.insert(
                conn_id.clone(),
                ConnectionRecord {
                    name,
                    connector_id,
                },
            );
        }
    }

    let v2 = ConnectionsFileV2 {
        version: 2,
        connections: migrated_map,
    };
    let mut json = serde_json::to_string_pretty(&v2)
        .map_err(|e| format!("failed to serialize migrated connections.json: {e}"))?;
    json.push('\n');
    write_text_atomic(connections_path, &json)?;
    Ok(v2)
}

/// `connections.json` の v2 テンプレート生成および v1（平文トークン保有）→ v2（秘密分離）自動移行を行う。
pub fn migrate_and_ensure_connections(
    connections_path: &Path,
    cred_store: &dyn CredentialStore,
) -> Result<ConnectionsFileV2, String> {
    let _guard = connections_lock()
        .lock()
        .map_err(|e| format!("connections lock poisoned: {e}"))?;
    migrate_and_ensure_connections_unlocked(connections_path, cred_store)
}

/// アプリ起動時に `connections.json` v2 の作成・v1 からの自動移行を実行する。
pub fn migrate_and_ensure_connections_for_app(app: &AppHandle) -> Result<PathBuf, String> {
    let path = connections_file_path(app)?;
    let cred_store = Win32CredentialStore;
    migrate_and_ensure_connections(&path, &cred_store)?;
    Ok(path)
}

pub fn build_request_headers(
    connector: &ConnectorManifest,
    secret: &str,
) -> Result<HeaderMap, String> {
    let mut headers = HeaderMap::new();

    let auth_header_name_lower = match &connector.auth {
        AuthConfig::Bearer { .. } => "authorization".to_string(),
        AuthConfig::ApiKeyHeader { header_name, .. } => header_name.to_ascii_lowercase(),
    };

    for (name, val) in &connector.headers {
        if name.eq_ignore_ascii_case("authorization")
            || name.eq_ignore_ascii_case(&auth_header_name_lower)
        {
            continue;
        }
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| format!("invalid header name '{name}' in connector '{}': {e}", connector.id))?;
        let header_val = HeaderValue::from_str(val)
            .map_err(|_| format!("invalid header value for '{name}' in connector '{}'", connector.id))?;
        headers.insert(header_name, header_val);
    }

    match &connector.auth {
        AuthConfig::Bearer { .. } => {
            let auth_value = HeaderValue::from_str(&format!("Bearer {secret}"))
                .map_err(|_| "invalid secret format in Windows Credential Manager".to_string())?;
            headers.insert(AUTHORIZATION, auth_value);
        }
        AuthConfig::ApiKeyHeader {
            header_name,
            prefix,
            ..
        } => {
            let hname = HeaderName::from_bytes(header_name.as_bytes())
                .map_err(|e| format!("invalid auth header name '{header_name}': {e}"))?;
            let raw_val = match prefix.as_deref() {
                Some(p) => format!("{p}{secret}"),
                None => secret.to_string(),
            };
            let hval = HeaderValue::from_str(&raw_val)
                .map_err(|_| "invalid secret format in Windows Credential Manager".to_string())?;
            headers.insert(hname, hval);
        }
    }

    Ok(headers)
}

pub struct PreparedHttpRequest {
    pub method: reqwest::Method,
    pub target_url: String,
    pub headers: HeaderMap,
}

/// テスト可能な接続解決・権限積集合・資格情報取得・ヘッダ生成パイプライン。
pub fn prepare_connect_request(
    html_path: &Path,
    connections_path: &Path,
    connectors: &BTreeMap<String, ConnectorManifest>,
    cred_store: &dyn CredentialStore,
    request: &ConnectRequest,
) -> Result<PreparedHttpRequest, String> {
    let widget_manifest = load_widget_manifest_for_html(html_path)?
        .ok_or_else(|| "widget manifest not found: DD.connect is disabled in restricted mode".to_string())?;

    let conns_v2 = migrate_and_ensure_connections(connections_path, cred_store)?;
    let conn_record = conns_v2.connections.get(&request.connection).ok_or_else(|| {
        format!(
            "connection '{}' is not configured in connections.json",
            request.connection
        )
    })?;

    let connector = connectors.get(&conn_record.connector_id).ok_or_else(|| {
        format!(
            "connector '{}' not found for connection '{}'",
            conn_record.connector_id, request.connection
        )
    })?;

    verify_endpoint_intersection(
        connector,
        &widget_manifest,
        &request.connection,
        &request.method,
        &request.path,
    )?;

    let cred_key = connector.auth.credential_key();
    let target_name = credential_target_name(&request.connection, cred_key);
    let secret = cred_store
        .get_secret(&target_name)?
        .map(|s| s.trim().to_string())
        .filter(|s| !is_dummy_or_empty_token(s))
        .ok_or_else(|| {
            format!(
                "接続 '{}' の認証情報 ({}) が Windows 資格情報マネージャーに未設定またはダミー値です (ターゲット: {})",
                request.connection, cred_key, target_name
            )
        })?;

    let method_upper = request.method.trim().to_ascii_uppercase();
    let method = reqwest::Method::from_bytes(method_upper.as_bytes())
        .map_err(|e| format!("invalid HTTP method: {e}"))?;
    let target_url = format!("{}{}", connector.base_url.trim_end_matches('/'), request.path);
    let headers = build_request_headers(connector, &secret)?;

    Ok(PreparedHttpRequest {
        method,
        target_url,
        headers,
    })
}

fn resolve_widget_html_path(app: &AppHandle, widget: &WidgetConfig) -> PathBuf {
    let entry_file = PathBuf::from(&widget.source);
    if entry_file.is_file() {
        return entry_file;
    }
    if let Some(file_name) = entry_file.file_name() {
        if let Some(samples_dir) = crate::resolve_samples_dir(app) {
            let fallback = samples_dir.join(file_name);
            if fallback.is_file() {
                return fallback;
            }
        }
    }
    entry_file
}

fn caller_file_widget(
    app: &AppHandle,
    window: &WebviewWindow,
    ipc_req: &tauri::ipc::Request<'_>,
) -> Result<WidgetConfig, String> {
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
    let widget_id = verify_file_widget_caller_and_origin(window.label(), origin_header, &cfg.widgets)?;
    cfg.widgets
        .iter()
        .find(|w| w.id == widget_id)
        .cloned()
        .ok_or_else(|| format!("widget not found: {widget_id}"))
}

#[tauri::command]
pub fn dd_storage_get(
    app: AppHandle,
    window: WebviewWindow,
    ipc_req: tauri::ipc::Request<'_>,
    key: String,
) -> Result<Value, String> {
    let widget = caller_file_widget(&app, &window, &ipc_req)?;
    let storage_dir = widget_storage_dir(&app)?;
    read_widget_storage_value(&storage_dir, &widget.id, &key)
}

#[tauri::command]
pub fn dd_storage_set(
    app: AppHandle,
    window: WebviewWindow,
    ipc_req: tauri::ipc::Request<'_>,
    key: String,
    value: Value,
) -> Result<(), String> {
    let widget = caller_file_widget(&app, &window, &ipc_req)?;
    let storage_dir = widget_storage_dir(&app)?;
    write_widget_storage_value(&storage_dir, &widget.id, &key, value)
}

#[tauri::command]
pub async fn dd_connect_request(
    app: AppHandle,
    window: WebviewWindow,
    ipc_req: tauri::ipc::Request<'_>,
    request: ConnectRequest,
) -> Result<ConnectResponse, String> {
    let widget = caller_file_widget(&app, &window, &ipc_req)?;
    let html_path = resolve_widget_html_path(&app, &widget);
    let conn_path = connections_file_path(&app)?;
    let connectors = load_connectors_for_app(&app);
    let cred_store = Win32CredentialStore;

    let prepared =
        prepare_connect_request(&html_path, &conn_path, &connectors, &cred_store, &request)?;

    let mut req_builder = http_client()
        .request(prepared.method, &prepared.target_url)
        .headers(prepared.headers);
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

fn collect_connector_credential_keys(connector: &ConnectorManifest) -> (Vec<String>, Vec<String>) {
    let mut all_keys = Vec::new();
    let mut seen_all = HashSet::new();
    let auth_key = connector.auth.credential_key().to_string();
    seen_all.insert(auth_key.clone());
    all_keys.push(auth_key.clone());

    let mut required_keys = Vec::new();
    let mut seen_req = HashSet::new();
    seen_req.insert(auth_key.clone());
    required_keys.push(auth_key);

    for field in &connector.credential_schema {
        if seen_all.insert(field.key.clone()) {
            all_keys.push(field.key.clone());
        }
        if field.required && seen_req.insert(field.key.clone()) {
            required_keys.push(field.key.clone());
        }
    }

    (all_keys, required_keys)
}

fn build_connection_summary(
    id: &str,
    record: &ConnectionRecord,
    connectors: &BTreeMap<String, ConnectorManifest>,
    cred_store: &dyn CredentialStore,
) -> Result<ConnectionSummaryDto, String> {
    if let Some(connector) = connectors.get(&record.connector_id) {
        let (all_keys, required_keys) = collect_connector_credential_keys(connector);
        let mut configured_keys = Vec::new();
        for key in &all_keys {
            let target = credential_target_name(id, key);
            if let Some(secret) = cred_store.get_secret(&target)? {
                if !is_dummy_or_empty_token(&secret) {
                    configured_keys.push(key.clone());
                }
            }
        }
        let has_secret = !required_keys.is_empty()
            && required_keys
                .iter()
                .all(|req| configured_keys.iter().any(|cfg_key| cfg_key == req));

        Ok(ConnectionSummaryDto {
            id: id.to_string(),
            name: record.name.clone(),
            connector_id: record.connector_id.clone(),
            connector_name: connector.name.clone(),
            official: connector.official,
            base_url: connector.base_url.clone(),
            allowed_hosts: connector.allowed_hosts.clone(),
            has_secret,
            configured_keys,
        })
    } else {
        Ok(ConnectionSummaryDto {
            id: id.to_string(),
            name: record.name.clone(),
            connector_id: record.connector_id.clone(),
            connector_name: record.connector_id.clone(),
            official: false,
            base_url: String::new(),
            allowed_hosts: Vec::new(),
            has_secret: false,
            configured_keys: Vec::new(),
        })
    }
}

pub fn summarize_connectors(
    connectors: &BTreeMap<String, ConnectorManifest>,
) -> Vec<ConnectorSummaryDto> {
    let mut list: Vec<ConnectorSummaryDto> = connectors
        .values()
        .map(|c| ConnectorSummaryDto {
            id: c.id.clone(),
            name: c.name.clone(),
            official: c.official,
            base_url: c.base_url.clone(),
            allowed_hosts: c.allowed_hosts.clone(),
            credential_schema: c.credential_schema.clone(),
            test_method: c.test.method.trim().to_ascii_uppercase(),
            test_path: c.test.path.clone(),
        })
        .collect();
    list.sort_by(|a, b| (!a.official).cmp(&(!b.official)).then_with(|| a.id.cmp(&b.id)));
    list
}

pub fn list_connections_with_store(
    connections_path: &Path,
    connectors: &BTreeMap<String, ConnectorManifest>,
    cred_store: &dyn CredentialStore,
) -> Result<Vec<ConnectionSummaryDto>, String> {
    let v2 = migrate_and_ensure_connections(connections_path, cred_store)?;
    let mut out = Vec::with_capacity(v2.connections.len());
    for (id, record) in &v2.connections {
        out.push(build_connection_summary(id, record, connectors, cred_store)?);
    }
    Ok(out)
}

pub fn save_connection_with_store(
    connections_path: &Path,
    connectors: &BTreeMap<String, ConnectorManifest>,
    cred_store: &dyn CredentialStore,
    input: &SaveConnectionInput,
) -> Result<ConnectionSummaryDto, String> {
    let id = input.id.trim();
    if !is_safe_identifier(id) {
        return Err(
            "接続 ID は空欄不可で、英数字・ハイフン(-)・アンダースコア(_)のみ使用できます"
                .to_string(),
        );
    }

    let connector_id = input.connector_id.trim();
    let connector = connectors
        .get(connector_id)
        .ok_or_else(|| format!("unknown connector_id: {connector_id}"))?;

    let name = if input.name.trim().is_empty() {
        connector.name.clone()
    } else {
        input.name.trim().to_string()
    };

    let mut pending_secrets: Vec<(String, String)> = Vec::new();
    for (key, val) in &input.credentials {
        if !is_safe_identifier(key) {
            return Err(format!("invalid credential key '{key}'"));
        }
        let trimmed = val.trim();
        if !trimmed.is_empty() {
            if is_dummy_or_empty_token(trimmed) {
                return Err(
                    "ダミートークンは保存できません。実際のトークンを入力してください".to_string(),
                );
            }
            pending_secrets.push((key.clone(), trimmed.to_string()));
        }
    }

    let _guard = connections_lock()
        .lock()
        .map_err(|e| format!("connections lock poisoned: {e}"))?;

    let mut v2 = migrate_and_ensure_connections_unlocked(connections_path, cred_store)?;

    let (_, required_keys) = collect_connector_credential_keys(connector);
    for req_key in &required_keys {
        let provided_now = pending_secrets.iter().any(|(k, _)| k == req_key);
        if !provided_now {
            let target = credential_target_name(id, req_key);
            let existing_valid = cred_store
                .get_secret(&target)?
                .map(|s| !is_dummy_or_empty_token(&s))
                .unwrap_or(false);
            if !existing_valid {
                return Err("必須の認証情報（トークン等）を入力してください".to_string());
            }
        }
    }

    for (key, secret) in &pending_secrets {
        let target = credential_target_name(id, key);
        cred_store.set_secret(&target, secret)?;
    }

    for req_key in &required_keys {
        let target = credential_target_name(id, req_key);
        let valid_after = cred_store
            .get_secret(&target)?
            .map(|s| !is_dummy_or_empty_token(&s))
            .unwrap_or(false);
        if !valid_after {
            return Err("必須の認証情報（トークン等）を入力してください".to_string());
        }
    }

    let record = ConnectionRecord {
        name,
        connector_id: connector_id.to_string(),
    };
    v2.connections.insert(id.to_string(), record.clone());

    let mut json = serde_json::to_string_pretty(&v2)
        .map_err(|e| format!("failed to serialize connections.json: {e}"))?;
    json.push('\n');
    write_text_atomic(connections_path, &json)?;

    build_connection_summary(id, &record, connectors, cred_store)
}

pub fn delete_connection_with_store(
    connections_path: &Path,
    connectors: &BTreeMap<String, ConnectorManifest>,
    cred_store: &dyn CredentialStore,
    id: &str,
) -> Result<(), String> {
    let trimmed_id = id.trim();
    if !is_safe_identifier(trimmed_id) {
        return Err("invalid connection id".to_string());
    }

    let removed_record = {
        let _guard = connections_lock()
            .lock()
            .map_err(|e| format!("connections lock poisoned: {e}"))?;
        let mut v2 = migrate_and_ensure_connections_unlocked(connections_path, cred_store)?;
        let Some(removed) = v2.connections.remove(trimmed_id) else {
            return Err(format!("connection '{trimmed_id}' not found"));
        };
        let mut json = serde_json::to_string_pretty(&v2)
            .map_err(|e| format!("failed to serialize connections.json: {e}"))?;
        json.push('\n');
        write_text_atomic(connections_path, &json)?;
        removed
    };

    let mut keys_to_delete = BTreeSet::new();
    keys_to_delete.insert("token".to_string());
    if let Some(connector) = connectors.get(&removed_record.connector_id) {
        let (all_keys, _) = collect_connector_credential_keys(connector);
        for k in all_keys {
            keys_to_delete.insert(k);
        }
    }

    for key in keys_to_delete {
        let target = credential_target_name(trimmed_id, &key);
        cred_store.delete_secret(&target)?;
    }

    Ok(())
}

pub fn evaluate_test_response(
    connection_id: &str,
    connector_id: &str,
    status: u16,
    body_text: &str,
) -> TestConnectionResultDto {
    let parsed = if body_text.trim().is_empty() {
        None
    } else {
        serde_json::from_str::<Value>(body_text).ok()
    };

    if (200..300).contains(&status) {
        let summary = format!("接続成功 (HTTP {status})");
        let detail = if let Some(val) = &parsed {
            let name = val
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let kind = val
                .get("type")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());
            let obj_id = val
                .get("id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty());

            match (name, kind, obj_id) {
                (Some(n), Some(k), _) if connector_id == "notion" => {
                    format!("Bot名: {n} (type: {k})")
                }
                (Some(n), Some(k), _) => format!("{n} (type: {k})"),
                (Some(n), None, _) => n.to_string(),
                (None, _, Some(oid)) => format!("ID: {oid}"),
                _ => "疎通確認に成功しました。".to_string(),
            }
        } else {
            "疎通確認に成功しました。".to_string()
        };

        let hint = if connector_id == "notion" {
            Some(
                "DB を参照するには、Notion 側の対象データベースの「⋯」→「コネクト」でこのインテグレーションを追加してください。"
                    .to_string(),
            )
        } else {
            None
        };

        return TestConnectionResultDto {
            connection_id: connection_id.to_string(),
            connector_id: connector_id.to_string(),
            ok: true,
            status,
            summary,
            detail,
            hint,
        };
    }

    let code = parsed
        .as_ref()
        .and_then(|v| v.get("code"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let message = parsed
        .as_ref()
        .and_then(|v| v.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("");

    let detail = match (!code.is_empty(), !message.is_empty()) {
        (true, true) => format!("[{code}] {message}"),
        (false, true) => message.to_string(),
        (true, false) => format!("[{code}]"),
        (false, false) => format!("HTTP {status} エラーが返されました。"),
    };

    let hint = if status == 401 || code == "unauthorized" {
        Some(
            "トークンが無効または取り消されている可能性があります。「編集」から正しいトークンに差し替えてください。"
                .to_string(),
        )
    } else if status == 404 || code == "object_not_found" {
        Some(
            "対象のリソース／データベースが見つかりません。Notion 側の対象ページ・DB でこのインテグレーションが共有（コネクト追加）されているか確認してください。"
                .to_string(),
        )
    } else if status == 403 || code == "restricted_resource" {
        Some("インテグレーションの権限（Read content 等）が不足している可能性があります。".to_string())
    } else {
        None
    };

    TestConnectionResultDto {
        connection_id: connection_id.to_string(),
        connector_id: connector_id.to_string(),
        ok: false,
        status,
        summary: format!("接続失敗 (HTTP {status})"),
        detail,
        hint,
    }
}

#[tauri::command]
pub fn list_connectors(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Vec<ConnectorSummaryDto>, String> {
    verify_manager_caller(window.label())?;
    let connectors = load_connectors_for_app(&app);
    Ok(summarize_connectors(&connectors))
}

#[tauri::command]
pub fn list_connections(
    app: AppHandle,
    window: WebviewWindow,
) -> Result<Vec<ConnectionSummaryDto>, String> {
    verify_manager_caller(window.label())?;
    let conn_path = connections_file_path(&app)?;
    let connectors = load_connectors_for_app(&app);
    list_connections_with_store(&conn_path, &connectors, &Win32CredentialStore)
}

#[tauri::command]
pub fn save_connection(
    app: AppHandle,
    window: WebviewWindow,
    input: SaveConnectionInput,
) -> Result<ConnectionSummaryDto, String> {
    verify_manager_caller(window.label())?;
    let conn_path = connections_file_path(&app)?;
    let connectors = load_connectors_for_app(&app);
    let summary = save_connection_with_store(&conn_path, &connectors, &Win32CredentialStore, &input)?;
    let _ = app.emit("connections-updated", ());
    Ok(summary)
}

#[tauri::command]
pub fn delete_connection(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
) -> Result<(), String> {
    verify_manager_caller(window.label())?;
    let conn_path = connections_file_path(&app)?;
    let connectors = load_connectors_for_app(&app);
    delete_connection_with_store(&conn_path, &connectors, &Win32CredentialStore, &id)?;
    let _ = app.emit("connections-updated", ());
    Ok(())
}

#[tauri::command]
pub async fn test_connection(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
) -> Result<TestConnectionResultDto, String> {
    verify_manager_caller(window.label())?;
    let conn_id = id.trim().to_string();
    if !is_safe_identifier(&conn_id) {
        return Err("invalid connection id".to_string());
    }

    let conn_path = connections_file_path(&app)?;
    let cred_store = Win32CredentialStore;
    let v2 = migrate_and_ensure_connections(&conn_path, &cred_store)?;
    let conn_record = v2
        .connections
        .get(&conn_id)
        .ok_or_else(|| format!("connection '{conn_id}' not found"))?;

    let connectors = load_connectors_for_app(&app);
    let connector = connectors.get(&conn_record.connector_id).ok_or_else(|| {
        format!(
            "connector '{}' not found for connection '{conn_id}'",
            conn_record.connector_id
        )
    })?;

    let cred_key = connector.auth.credential_key();
    let target_name = credential_target_name(&conn_id, cred_key);
    let Some(secret) = cred_store
        .get_secret(&target_name)?
        .map(|s| s.trim().to_string())
        .filter(|s| !is_dummy_or_empty_token(s))
    else {
        return Ok(TestConnectionResultDto {
            connection_id: conn_id,
            connector_id: connector.id.clone(),
            ok: false,
            status: 0,
            summary: "認証情報未設定".to_string(),
            detail: "Windows 資格情報マネージャーにトークンが登録されていません。".to_string(),
            hint: Some("「編集」からトークンを設定してください。".to_string()),
        });
    };

    verify_request_path_safety(&connector.test.path)?;
    let target_url = format!(
        "{}{}",
        connector.base_url.trim_end_matches('/'),
        connector.test.path
    );
    let parsed_target =
        url::Url::parse(&target_url).map_err(|e| format!("invalid target URL: {e}"))?;
    if parsed_target.scheme() != "https" {
        return Err("target URL must use https scheme".to_string());
    }
    let Some(host) = parsed_target.host_str() else {
        return Err("target URL is missing host".to_string());
    };
    if !connector
        .allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
    {
        return Err(format!(
            "target host '{host}' is not allowed for connector '{}'",
            connector.id
        ));
    }

    let method_upper = connector.test.method.trim().to_ascii_uppercase();
    let method = reqwest::Method::from_bytes(method_upper.as_bytes())
        .map_err(|e| format!("invalid HTTP method: {e}"))?;
    let headers = build_request_headers(connector, &secret)?;

    let response = match http_client()
        .request(method, &target_url)
        .headers(headers)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            return Ok(TestConnectionResultDto {
                connection_id: conn_id,
                connector_id: connector.id.clone(),
                ok: false,
                status: 0,
                summary: "通信エラー".to_string(),
                detail: format!("HTTP request failed: {}", e.without_url()),
                hint: Some("ネットワーク接続やプロキシ設定を確認してください。".to_string()),
            });
        }
    };

    let status = response.status().as_u16();
    let body_text = response
        .text()
        .await
        .map_err(|e| format!("failed to read HTTP response body: {e}"))?;

    Ok(evaluate_test_response(
        &conn_id,
        &connector.id,
        status,
        &body_text,
    ))
}

fn default_value_for_schema_field(field: &ConfigFieldSchema) -> Value {
    if let Some(default_val) = &field.default {
        if !default_val.is_null() {
            return default_val.clone();
        }
    }
    match field.field_type.as_str() {
        "boolean" => Value::Bool(false),
        _ => Value::String(String::new()),
    }
}

fn value_matches_schema_type(val: &Value, field_type: &str) -> bool {
    match field_type {
        "string" => val.is_string(),
        "boolean" => val.is_boolean(),
        _ => false,
    }
}

fn build_effective_config_map(
    manifest: &WidgetManifest,
    saved_map: &Map<String, Value>,
) -> Map<String, Value> {
    let mut effective = Map::new();
    for field in &manifest.config_schema {
        let val = match saved_map.get(&field.key) {
            Some(saved) if value_matches_schema_type(saved, &field.field_type) => saved.clone(),
            _ => default_value_for_schema_field(field),
        };
        effective.insert(field.key.clone(), val);
    }
    effective
}

/// ウィジェット固有の `widget_config/{widget_id}.json` と `config_schema` デフォルト値を統合した設定オブジェクトを返す。
pub fn read_effective_widget_config(
    config_dir: &Path,
    html_path: &Path,
    widget_id: &str,
) -> Result<Value, String> {
    validate_widget_id(widget_id)?;
    let Some(manifest) = load_widget_manifest_for_html(html_path)? else {
        return Ok(Value::Object(Map::new()));
    };

    let path = config_dir.join(format!("{widget_id}.json"));
    let _guard = widget_config_lock()
        .lock()
        .map_err(|e| format!("widget config lock poisoned: {e}"))?;
    let saved_map = read_storage_map(&path)?;
    Ok(Value::Object(build_effective_config_map(
        &manifest,
        &saved_map,
    )))
}

/// 管理画面フォーム用に `WidgetConfigFormDto` を構築する。
pub fn read_widget_config_form_for_widget(
    config_dir: &Path,
    widget: &WidgetConfig,
    html_path: &Path,
) -> Result<WidgetConfigFormDto, String> {
    if widget.source_type != SourceType::File {
        return Ok(WidgetConfigFormDto {
            widget_id: widget.id.clone(),
            has_manifest: false,
            manifest_id: None,
            manifest_name: None,
            schema: Vec::new(),
            values: Map::new(),
        });
    }

    let Some(manifest) = load_widget_manifest_for_html(html_path)? else {
        return Ok(WidgetConfigFormDto {
            widget_id: widget.id.clone(),
            has_manifest: false,
            manifest_id: None,
            manifest_name: None,
            schema: Vec::new(),
            values: Map::new(),
        });
    };

    validate_widget_id(&widget.id)?;
    let path = config_dir.join(format!("{}.json", widget.id));
    let _guard = widget_config_lock()
        .lock()
        .map_err(|e| format!("widget config lock poisoned: {e}"))?;
    let saved_map = read_storage_map(&path)?;
    let effective = build_effective_config_map(&manifest, &saved_map);

    Ok(WidgetConfigFormDto {
        widget_id: widget.id.clone(),
        has_manifest: true,
        manifest_id: Some(manifest.id),
        manifest_name: Some(manifest.name),
        schema: manifest.config_schema,
        values: effective,
    })
}

/// `config_schema` 定義キーのみを型検証した上で `widget_config/{widget_id}.json` へ原子的に保存する。
pub fn save_widget_config_values(
    config_dir: &Path,
    html_path: &Path,
    widget_id: &str,
    raw_values: &Map<String, Value>,
) -> Result<WidgetConfigFormDto, String> {
    validate_widget_id(widget_id)?;
    let manifest = load_widget_manifest_for_html(html_path)?.ok_or_else(|| {
        "widget manifest not found: widget config is not available".to_string()
    })?;

    let path = config_dir.join(format!("{widget_id}.json"));
    let _guard = widget_config_lock()
        .lock()
        .map_err(|e| format!("widget config lock poisoned: {e}"))?;
    let existing_map = read_storage_map(&path)?;

    let mut next_map = Map::new();
    for field in &manifest.config_schema {
        let next_val = if let Some(val) = raw_values.get(&field.key) {
            if !value_matches_schema_type(val, &field.field_type) {
                return Err(format!(
                    "config field '{}' must be of type '{}'",
                    field.key, field.field_type
                ));
            }
            val.clone()
        } else if let Some(prev) = existing_map.get(&field.key) {
            if value_matches_schema_type(prev, &field.field_type) {
                prev.clone()
            } else {
                default_value_for_schema_field(field)
            }
        } else {
            default_value_for_schema_field(field)
        };
        next_map.insert(field.key.clone(), next_val);
    }

    let mut json = serde_json::to_string_pretty(&next_map)
        .map_err(|e| format!("failed to serialize widget config: {e}"))?;
    json.push('\n');
    write_text_atomic(&path, &json)?;

    Ok(WidgetConfigFormDto {
        widget_id: widget_id.to_string(),
        has_manifest: true,
        manifest_id: Some(manifest.id),
        manifest_name: Some(manifest.name),
        schema: manifest.config_schema,
        values: next_map,
    })
}

/// `DD.openUrl` で起動可能な URL が `http://` または `https://`（ホスト名あり）であることを検証する。
pub fn validate_open_url_target(raw_url: &str) -> Result<url::Url, String> {
    let trimmed = raw_url.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_control()) {
        return Err("invalid URL: empty or contains control characters".to_string());
    }
    let parsed = url::Url::parse(trimmed).map_err(|e| format!("invalid URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!(
            "unsupported URL scheme '{}' (only http and https are allowed)",
            parsed.scheme()
        ));
    }
    if parsed.host_str().is_none() {
        return Err("invalid URL: missing host".to_string());
    }
    Ok(parsed)
}

fn find_widget_by_id(app: &AppHandle, widget_id: &str) -> Result<WidgetConfig, String> {
    let state = app
        .try_state::<Arc<ConfigState>>()
        .ok_or_else(|| "ConfigState not initialized".to_string())?;
    let cfg = state
        .config
        .lock()
        .map_err(|e| format!("config lock poisoned: {e}"))?;
    cfg.widgets
        .iter()
        .find(|w| w.id == widget_id)
        .cloned()
        .ok_or_else(|| format!("widget not found: {widget_id}"))
}

#[tauri::command]
pub fn dd_config_get(
    app: AppHandle,
    window: WebviewWindow,
    ipc_req: tauri::ipc::Request<'_>,
) -> Result<Value, String> {
    let widget = caller_file_widget(&app, &window, &ipc_req)?;
    let html_path = resolve_widget_html_path(&app, &widget);
    let cfg_dir = widget_config_dir(&app)?;
    read_effective_widget_config(&cfg_dir, &html_path, &widget.id)
}

#[tauri::command]
pub fn dd_open_url(
    app: AppHandle,
    window: WebviewWindow,
    ipc_req: tauri::ipc::Request<'_>,
    url: String,
) -> Result<(), String> {
    let _widget = caller_file_widget(&app, &window, &ipc_req)?;
    let parsed = validate_open_url_target(&url)?;
    tauri_plugin_opener::open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| format!("failed to open URL: {e}"))
}

#[tauri::command]
pub fn get_widget_config_form(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
) -> Result<WidgetConfigFormDto, String> {
    verify_manager_caller(window.label())?;
    let widget = find_widget_by_id(&app, &id)?;
    let html_path = resolve_widget_html_path(&app, &widget);
    let cfg_dir = widget_config_dir(&app)?;
    read_widget_config_form_for_widget(&cfg_dir, &widget, &html_path)
}

#[tauri::command]
pub fn save_widget_config(
    app: AppHandle,
    window: WebviewWindow,
    id: String,
    values: Map<String, Value>,
) -> Result<WidgetConfigFormDto, String> {
    verify_manager_caller(window.label())?;
    let widget = find_widget_by_id(&app, &id)?;
    if widget.source_type != SourceType::File {
        return Err(
            "widget config is only supported for local HTML widgets (SourceType::File)".to_string(),
        );
    }
    let html_path = resolve_widget_html_path(&app, &widget);
    let cfg_dir = widget_config_dir(&app)?;
    let dto = save_widget_config_values(&cfg_dir, &html_path, &widget.id, &values)?;
    crate::widget::reload_widget_window(&app, &id);
    Ok(dto)
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

    fn read_only_notion_widget_manifest() -> WidgetManifest {
        parse_and_validate_widget_manifest(
            r#"{
  "id": "notion-spike",
  "name": "Notion Spike",
  "version": "0.2.0",
  "entry": "notion-spike.html",
  "connections": [
    {
      "connection": "notion",
      "connector": "notion",
      "endpoints": [
        { "method": "GET", "path": "/v1/users/me" },
        { "method": "GET", "path": "/v1/databases/*" },
        { "method": "POST", "path": "/v1/data_sources/*/query" }
      ]
    }
  ]
}"#,
        )
        .unwrap()
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

    #[test]
    fn widget_storage_isolates_by_widget_id_and_deletes_on_null() {
        let dir = std::env::temp_dir().join(format!("dd_storage_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        assert_eq!(
            read_widget_storage_value(&dir, "w1", "notion:database_id").unwrap(),
            Value::Null
        );

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

        write_widget_storage_value(&dir, "w1", "notion:data_source_id:db-111", json!("ds-aaa"))
            .unwrap();
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

        assert!(read_widget_storage_value(&dir, "../evil", "k").is_err());
        assert!(write_widget_storage_value(&dir, "a/b", "k", json!(1)).is_err());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn connector_manifest_validation_and_loader_protection() {
        // 同梱 connectors/notion.json が正しく検証通過すること
        let notion = parse_and_validate_connector(BUILTIN_NOTION_CONNECTOR_JSON).unwrap();
        assert_eq!(notion.id, "notion");
        assert!(notion.official);
        assert_eq!(notion.base_url, "https://api.notion.com");
        assert_eq!(notion.allowed_hosts, vec!["api.notion.com"]);
        assert_eq!(
            notion.auth,
            AuthConfig::Bearer {
                credential_key: "token".to_string()
            }
        );
        assert_eq!(
            notion.headers.get("Notion-Version").map(String::as_str),
            Some("2025-09-03")
        );
        assert!(notion
            .endpoints
            .iter()
            .any(|e| e.method == "PATCH" && e.path == "/v1/pages/*"));

        // ApiKeyHeader 方式もパース・ヘッダ構築できること
        let api_key_conn = parse_and_validate_connector(
            r#"{
  "id": "custom_weather",
  "name": "Weather",
  "official": true,
  "base_url": "https://api.weather.example/",
  "allowed_hosts": ["api.weather.example"],
  "auth": { "type": "api_key_header", "header_name": "X-Api-Key", "credential_key": "api_key", "prefix": "Key " },
  "headers": { "Accept": "application/json" },
  "test": { "method": "GET", "path": "/v1/ping" },
  "endpoints": [{ "method": "GET", "path": "/v1/forecast/*" }]
}"#,
        )
        .unwrap();
        let headers = build_request_headers(&api_key_conn, "secret_abc").unwrap();
        assert_eq!(headers.get("x-api-key").unwrap(), "Key secret_abc");
        assert_eq!(headers.get("accept").unwrap(), "application/json");

        // スキーマ違反（http://・allowed_hosts不一致・ワイルドカードホスト・authorizationヘッダ・トラバーサル・不正JSON）は拒否
        assert!(parse_and_validate_connector("{ invalid json").is_err());
        assert!(parse_and_validate_connector(
            r#"{"id":"bad","name":"Bad","base_url":"http://api.example.com","allowed_hosts":["api.example.com"],"auth":{"type":"bearer","credential_key":"token"},"test":{"method":"GET","path":"/ping"}}"#
        ).is_err());
        assert!(parse_and_validate_connector(
            r#"{"id":"bad","name":"Bad","base_url":"https://evil.com","allowed_hosts":["api.example.com"],"auth":{"type":"bearer","credential_key":"token"},"test":{"method":"GET","path":"/ping"}}"#
        ).is_err());
        assert!(parse_and_validate_connector(
            r#"{"id":"bad","name":"Bad","base_url":"https://api.example.com","allowed_hosts":["*.example.com"],"auth":{"type":"bearer","credential_key":"token"},"test":{"method":"GET","path":"/ping"}}"#
        ).is_err());
        assert!(parse_and_validate_connector(
            r#"{"id":"bad","name":"Bad","base_url":"https://api.example.com","allowed_hosts":["api.example.com"],"auth":{"type":"bearer","credential_key":"token"},"headers":{"Authorization":"Bearer x"},"test":{"method":"GET","path":"/ping"}}"#
        ).is_err());
        assert!(parse_and_validate_connector(
            r#"{"id":"bad","name":"Bad","base_url":"https://api.example.com","allowed_hosts":["api.example.com"],"auth":{"type":"bearer","credential_key":"token"},"test":{"method":"GET","path":"/v1/*"}}"#
        ).is_err());

        // ユーザー定義フォルダ: 不正 JSON 無視、official=false 強制、公式 ID ("notion") 上書き拒否
        let user_dir =
            std::env::temp_dir().join(format!("dd_user_conn_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&user_dir).unwrap();
        fs::write(user_dir.join("broken.json"), "{ not valid json").unwrap();
        fs::write(
            user_dir.join("hijack_notion.json"),
            r#"{"id":"notion","name":"Evil Notion","official":true,"base_url":"https://evil.example.com","allowed_hosts":["evil.example.com"],"auth":{"type":"bearer","credential_key":"token"},"test":{"method":"GET","path":"/v1/users/me"}}"#,
        )
        .unwrap();
        fs::write(
            user_dir.join("my_custom.json"),
            r#"{"id":"my_custom","name":"My Custom","official":true,"base_url":"https://custom.example.com","allowed_hosts":["custom.example.com"],"auth":{"type":"bearer","credential_key":"token"},"test":{"method":"GET","path":"/v1/status"},"endpoints":[{"method":"GET","path":"/v1/items/*"}]}"#,
        )
        .unwrap();

        let registry = load_connectors_from_dirs(&[], Some(&user_dir));
        let loaded_notion = registry.get("notion").unwrap();
        assert_eq!(loaded_notion.base_url, "https://api.notion.com");
        assert!(loaded_notion.official);

        let loaded_custom = registry.get("my_custom").unwrap();
        assert!(!loaded_custom.official);
        assert_eq!(loaded_custom.base_url, "https://custom.example.com");

        let _ = fs::remove_dir_all(&user_dir);
    }

    #[test]
    fn widget_manifest_loader_priority_entry_and_restricted_mode() {
        let dir = std::env::temp_dir().join(format!("dd_wmanifest_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        let html_a = dir.join("alpha.html");
        let html_b = dir.join("beta.html");
        fs::write(&html_a, "<html></html>").unwrap();
        fs::write(&html_b, "<html></html>").unwrap();

        // manifest 不在時は Ok(None)（制限モード）
        assert!(load_widget_manifest_for_html(&html_a).unwrap().is_none());

        // manifest.json の entry が一致する alpha のみ適用され、beta は対象外 (Ok(None))
        fs::write(
            dir.join("manifest.json"),
            r#"{"id":"shared","name":"Shared Alpha","entry":"alpha.html","connections":[{"connection":"notion","endpoints":[{"method":"GET","path":"/v1/users/me"}]}]}"#,
        )
        .unwrap();
        assert_eq!(
            load_widget_manifest_for_html(&html_a)
                .unwrap()
                .unwrap()
                .id,
            "shared"
        );
        assert!(load_widget_manifest_for_html(&html_b).unwrap().is_none());

        // <stem>.manifest.json は manifest.json より優先される
        fs::write(
            dir.join("alpha.manifest.json"),
            r#"{"id":"alpha-specific","name":"Alpha Specific","entry":"alpha.html","connections":[{"connection":"notion","endpoints":[{"method":"GET","path":"/v1/users/me"}]}]}"#,
        )
        .unwrap();
        assert_eq!(
            load_widget_manifest_for_html(&html_a)
                .unwrap()
                .unwrap()
                .id,
            "alpha-specific"
        );

        // 壊れた manifest は Err("invalid widget manifest: ...")
        fs::write(dir.join("beta.manifest.json"), "{ corrupted").unwrap();
        let err = load_widget_manifest_for_html(&html_b).unwrap_err();
        assert!(err.starts_with("invalid widget manifest:"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn endpoint_intersection_enforces_widget_and_connector_and_host_guards() {
        let notion = parse_and_validate_connector(BUILTIN_NOTION_CONNECTOR_JSON).unwrap();
        let widget = read_only_notion_widget_manifest();

        // 積集合で許可される 3 種
        assert!(verify_endpoint_intersection(&notion, &widget, "notion", "GET", "/v1/users/me").is_ok());
        assert!(
            verify_endpoint_intersection(&notion, &widget, "notion", "get", "/v1/databases/db-123")
                .is_ok()
        );
        assert!(verify_endpoint_intersection(
            &notion,
            &widget,
            "notion",
            "POST",
            "/v1/data_sources/ds-456/query"
        )
        .is_ok());

        // コネクタには PATCH /v1/pages/* があるが、ウィジェット manifest に無いため拒否
        let patch_err =
            verify_endpoint_intersection(&notion, &widget, "notion", "PATCH", "/v1/pages/test")
                .unwrap_err();
        assert!(patch_err.contains("widget manifest"));

        // 未宣言 connection 拒否
        let unconn_err =
            verify_endpoint_intersection(&notion, &widget, "slack", "GET", "/v1/users/me")
                .unwrap_err();
        assert_eq!(
            unconn_err,
            "connection 'slack' is not declared in widget manifest"
        );

        // ワイルドカード境界・トラバーサル・クエリ・絶対 URL・空セグメント拒否
        for bad_path in [
            "/v1/databases/",
            "/v1/databases/a/b",
            "/v1/databases/../users/me",
            "/v1/databases/db1?foo=bar",
            "/v1/databases/db1#hash",
            "/v1/databases//db1",
            "/v1/databases/%2e%2e",
            "https://evil.com/v1/users/me",
            "/v1/data_sources//query",
        ] {
            assert!(
                verify_endpoint_intersection(&notion, &widget, "notion", "GET", bad_path).is_err(),
                "expected rejection for path '{bad_path}'"
            );
        }

        // allowed_hosts 外のホスト改ざんに対する防衛チェック
        let mut tampered_notion = notion.clone();
        tampered_notion.base_url = "https://evil.example.com".to_string();
        assert!(verify_endpoint_intersection(
            &tampered_notion,
            &widget,
            "notion",
            "GET",
            "/v1/users/me"
        )
        .is_err());
    }

    #[test]
    fn connections_v1_migrates_real_tokens_and_strips_plaintext_and_dummy() {
        let dir = std::env::temp_dir().join(format!("dd_migrate_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let conn_path = dir.join("connections.json");
        let cred_store = InMemoryCredentialStore::default();

        // 1) 未作成時は v2 テンプレート（秘密なし）を生成
        let v2_init = migrate_and_ensure_connections(&conn_path, &cred_store).unwrap();
        assert_eq!(v2_init.version, 2);
        assert_eq!(
            v2_init.connections.get("notion"),
            Some(&ConnectionRecord {
                name: "Notion".to_string(),
                connector_id: "notion".to_string()
            })
        );
        let init_text = fs::read_to_string(&conn_path).unwrap();
        assert!(!init_text.contains("token"));

        // 2) v1 に実トークンがある場合 -> CredentialStore へ移行し、connections.json から平文トークン完全消去
        let real_secret = "ntn_live_secret_token_for_migration_test";
        fs::write(
            &conn_path,
            format!(
                r#"{{
  "version": 1,
  "connections": {{
    "notion": {{
      "base_url": "https://api.notion.com",
      "token": "{real_secret}",
      "headers": {{ "Notion-Version": "2025-09-03" }}
    }},
    "notion_dummy": {{
      "base_url": "https://api.notion.com",
      "token": "ntn_DUMMY_REPLACE_WITH_YOUR_NOTION_TOKEN",
      "headers": {{}}
    }}
  }}
}}"#
            ),
        )
        .unwrap();

        let migrated = migrate_and_ensure_connections(&conn_path, &cred_store).unwrap();
        assert_eq!(migrated.version, 2);
        assert_eq!(
            cred_store
                .get_secret("DesktopDashboards:connection:notion:token")
                .unwrap(),
            Some(real_secret.to_string())
        );
        // ダミートークンは CredentialStore に移行されない
        assert_eq!(
            cred_store
                .get_secret("DesktopDashboards:connection:notion_dummy:token")
                .unwrap(),
            None
        );

        let after_text = fs::read_to_string(&conn_path).unwrap();
        assert!(!after_text.contains(real_secret));
        assert!(!after_text.contains("ntn_DUMMY"));
        assert!(!after_text.contains("\"token\""));
        assert!(!after_text.contains("base_url"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepare_connect_request_handles_restricted_mode_and_secrets_safely() {
        let dir = std::env::temp_dir().join(format!("dd_prepare_test_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();

        let starter_html = dir.join("starter-template.html");
        let spike_html = dir.join("notion-spike.html");
        fs::write(&starter_html, "<html></html>").unwrap();
        fs::write(&spike_html, "<html></html>").unwrap();
        fs::write(
            dir.join("notion-spike.manifest.json"),
            r#"{
  "id": "notion-spike",
  "name": "Notion Spike",
  "version": "0.2.0",
  "entry": "notion-spike.html",
  "connections": [
    {
      "connection": "notion",
      "connector": "notion",
      "endpoints": [
        { "method": "GET", "path": "/v1/users/me" },
        { "method": "GET", "path": "/v1/databases/*" },
        { "method": "POST", "path": "/v1/data_sources/*/query" }
      ]
    }
  ]
}"#,
        )
        .unwrap();

        let conn_path = dir.join("connections.json");
        let connectors = load_connectors_from_dirs(&[], None);
        let cred_store = InMemoryCredentialStore::default();

        let get_me = ConnectRequest {
            connection: "notion".to_string(),
            method: "GET".to_string(),
            path: "/v1/users/me".to_string(),
            body: None,
        };

        // manifest 無しの HTML (starter-template.html) は制限モードで拒否
        let restricted_err =
            prepare_connect_request(&starter_html, &conn_path, &connectors, &cred_store, &get_me)
                .err()
                .unwrap();
        assert_eq!(
            restricted_err,
            "widget manifest not found: DD.connect is disabled in restricted mode"
        );

        // manifest ありでも資格情報未設定なら拒否（エラー文面にターゲット名あり・秘密なし）
        let missing_cred_err =
            prepare_connect_request(&spike_html, &conn_path, &connectors, &cred_store, &get_me)
                .err()
                .unwrap();
        assert!(missing_cred_err.contains("DesktopDashboards:connection:notion:token"));

        // 資格情報設定後は成功し、Authorization ヘッダと Notion-Version ヘッダが付与される
        let secret_val = "ntn_live_secret_xyz_999";
        cred_store
            .set_secret("DesktopDashboards:connection:notion:token", secret_val)
            .unwrap();
        let prepared =
            prepare_connect_request(&spike_html, &conn_path, &connectors, &cred_store, &get_me)
                .unwrap();
        assert_eq!(prepared.target_url, "https://api.notion.com/v1/users/me");
        assert_eq!(
            prepared.headers.get(AUTHORIZATION).unwrap(),
            &format!("Bearer {secret_val}")
        );
        assert_eq!(prepared.headers.get("notion-version").unwrap(), "2025-09-03");

        // PATCH /v1/pages/test は積集合で拒否
        let patch_req = ConnectRequest {
            connection: "notion".to_string(),
            method: "PATCH".to_string(),
            path: "/v1/pages/test".to_string(),
            body: Some(json!({})),
        };
        assert!(
            prepare_connect_request(&spike_html, &conn_path, &connectors, &cred_store, &patch_req)
                .is_err()
        );

        // delete_secret 後は再度未設定エラーとなる
        cred_store
            .delete_secret("DesktopDashboards:connection:notion:token")
            .unwrap();
        assert!(
            prepare_connect_request(&spike_html, &conn_path, &connectors, &cred_store, &get_me)
                .is_err()
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn utf16le_credential_blob_roundtrips_and_falls_back_to_utf8() {
        let secret = "ntn_秘密トークン_ABC_123";
        let utf16_bytes = encode_credential_blob_utf16le(secret);
        assert_eq!(decode_credential_blob(&utf16_bytes).unwrap(), secret);

        // 末尾 NUL 付き UTF-16LE も正しくトリムされる
        let mut with_nul = utf16_bytes.clone();
        with_nul.extend_from_slice(&0u16.to_le_bytes());
        assert_eq!(decode_credential_blob(&with_nul).unwrap(), secret);

        // 奇数長などの UTF-8 バイト列は UTF-8 フォールバックで読める
        assert_eq!(decode_credential_blob(b"abc").unwrap(), "abc");
    }

    #[test]
    fn verify_manager_caller_allows_only_manager() {
        assert!(verify_manager_caller("manager").is_ok());

        for bad in ["widget-123", "widget-file-widget-1", "", "other"] {
            let err = verify_manager_caller(bad).unwrap_err();
            assert_eq!(
                err,
                "connection management commands are only allowed from the manager window"
            );
        }
    }

    #[test]
    fn default_capability_has_connection_management_and_widgets_exclude_it() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let default_cap: Value = serde_json::from_str(
            &fs::read_to_string(manifest_dir.join("capabilities").join("default.json")).unwrap(),
        )
        .unwrap();
        let widgets_cap: Value = serde_json::from_str(
            &fs::read_to_string(manifest_dir.join("capabilities").join("widgets.json")).unwrap(),
        )
        .unwrap();
        let widgets_dd_cap: Value = serde_json::from_str(
            &fs::read_to_string(manifest_dir.join("capabilities").join("widgets-dd.json")).unwrap(),
        )
        .unwrap();

        let mgmt_perms = [
            "allow-list-connectors",
            "allow-list-connections",
            "allow-save-connection",
            "allow-delete-connection",
            "allow-test-connection",
        ];

        assert_eq!(default_cap.get("windows"), Some(&json!(["manager"])));
        let default_perms = default_cap
            .get("permissions")
            .and_then(Value::as_array)
            .unwrap();
        for perm in mgmt_perms {
            assert!(
                default_perms.iter().any(|p| p.as_str() == Some(perm)),
                "default.json missing {perm}"
            );
        }

        for (name, cap) in [("widgets.json", &widgets_cap), ("widgets-dd.json", &widgets_dd_cap)] {
            let perms = cap.get("permissions").and_then(Value::as_array).unwrap();
            for perm in mgmt_perms {
                assert!(
                    !perms.iter().any(|p| p.as_str() == Some(perm)),
                    "{name} must not expose {perm}"
                );
            }
        }
    }

    #[test]
    fn save_connection_input_debug_redacts_credentials() {
        let mut creds = BTreeMap::new();
        let secret = "ntn_super_secret_do_not_leak_42";
        creds.insert("token".to_string(), secret.to_string());
        let input = SaveConnectionInput {
            id: "notion_work".to_string(),
            name: "Notion Work".to_string(),
            connector_id: "notion".to_string(),
            credentials: creds,
        };
        let debug_str = format!("{input:?}");
        assert!(!debug_str.contains(secret));
        assert!(debug_str.contains("[REDACTED]"));
        assert!(debug_str.contains("notion_work"));
    }

    #[test]
    fn connection_management_save_list_update_delete_lifecycle() {
        let dir = std::env::temp_dir().join(format!("dd_conn_mgmt_{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let conn_path = dir.join("connections.json");
        let spike_html = dir.join("notion-spike.html");
        fs::write(&spike_html, "<html></html>").unwrap();
        fs::write(
            dir.join("notion-spike.manifest.json"),
            r#"{
  "id": "notion-spike",
  "name": "Notion Spike",
  "entry": "notion-spike.html",
  "connections": [
    {
      "connection": "*",
      "connector": "notion",
      "endpoints": [
        { "method": "GET", "path": "/v1/users/me" }
      ]
    }
  ]
}"#,
        )
        .unwrap();

        let connectors = load_connectors_from_dirs(&[], None);
        let cred_store = InMemoryCredentialStore::default();

        // 1) 必須トークン未入力・ダミートークンは拒否され、connections.json へ不完全な接続を作らない
        let dummy_input = SaveConnectionInput {
            id: "notion_work".to_string(),
            name: "Work Notion".to_string(),
            connector_id: "notion".to_string(),
            credentials: BTreeMap::from([("token".to_string(), "ntn_DUMMY_123".to_string())]),
        };
        assert!(
            save_connection_with_store(&conn_path, &connectors, &cred_store, &dummy_input)
                .is_err()
        );
        let empty_input = SaveConnectionInput {
            id: "notion_work".to_string(),
            name: "Work Notion".to_string(),
            connector_id: "notion".to_string(),
            credentials: BTreeMap::new(),
        };
        assert!(
            save_connection_with_store(&conn_path, &connectors, &cred_store, &empty_input)
                .is_err()
        );
        let initial_list =
            list_connections_with_store(&conn_path, &connectors, &cred_store).unwrap();
        assert!(!initial_list.iter().any(|c| c.id == "notion_work"));

        // 2) 有効トークンで新規保存
        let secret_1 = "ntn_work_secret_v1_111";
        let valid_input = SaveConnectionInput {
            id: "notion_work".to_string(),
            name: "Work Notion".to_string(),
            connector_id: "notion".to_string(),
            credentials: BTreeMap::from([("token".to_string(), secret_1.to_string())]),
        };
        let saved =
            save_connection_with_store(&conn_path, &connectors, &cred_store, &valid_input).unwrap();
        assert_eq!(saved.id, "notion_work");
        assert_eq!(saved.name, "Work Notion");
        assert!(saved.official);
        assert!(saved.has_secret);
        assert_eq!(saved.configured_keys, vec!["token".to_string()]);
        assert_eq!(saved.base_url, "https://api.notion.com");

        // 3) list_connections_with_store で一覧取得でき、connections.json には平文トークンなし
        let listed = list_connections_with_store(&conn_path, &connectors, &cred_store).unwrap();
        assert_eq!(listed.len(), 2);
        let conn_json_text = fs::read_to_string(&conn_path).unwrap();
        assert!(!conn_json_text.contains(secret_1));

        // 4) トークン空欄で編集保存すると既存トークンを維持しつつ名前のみ更新
        let rename_only = SaveConnectionInput {
            id: "notion_work".to_string(),
            name: "Work Notion Renamed".to_string(),
            connector_id: "notion".to_string(),
            credentials: BTreeMap::from([("token".to_string(), "".to_string())]),
        };
        let renamed =
            save_connection_with_store(&conn_path, &connectors, &cred_store, &rename_only).unwrap();
        assert_eq!(renamed.name, "Work Notion Renamed");
        assert!(renamed.has_secret);
        assert_eq!(
            cred_store
                .get_secret("DesktopDashboards:connection:notion_work:token")
                .unwrap(),
            Some(secret_1.to_string())
        );

        // 5) トークン差し替え更新
        let secret_2 = "ntn_work_secret_v2_222";
        let rotate_input = SaveConnectionInput {
            id: "notion_work".to_string(),
            name: "Work Notion Renamed".to_string(),
            connector_id: "notion".to_string(),
            credentials: BTreeMap::from([("token".to_string(), secret_2.to_string())]),
        };
        save_connection_with_store(&conn_path, &connectors, &cred_store, &rotate_input).unwrap();
        assert_eq!(
            cred_store
                .get_secret("DesktopDashboards:connection:notion_work:token")
                .unwrap(),
            Some(secret_2.to_string())
        );

        // 6) 削除すると connections.json と CredentialStore の両方から消え、prepare_connect_request も拒否
        delete_connection_with_store(&conn_path, &connectors, &cred_store, "notion_work").unwrap();
        assert_eq!(
            cred_store
                .get_secret("DesktopDashboards:connection:notion_work:token")
                .unwrap(),
            None
        );
        let req_deleted = ConnectRequest {
            connection: "notion_work".to_string(),
            method: "GET".to_string(),
            path: "/v1/users/me".to_string(),
            body: None,
        };
        let prep_err = prepare_connect_request(
            &spike_html,
            &conn_path,
            &connectors,
            &cred_store,
            &req_deleted,
        )
        .err()
        .unwrap();
        assert!(prep_err.contains("not configured in connections.json"));

        // 7) 残りの notion も削除し全件空にした後、migrate_and_ensure_connections / list_connections で勝手に復活しないこと
        delete_connection_with_store(&conn_path, &connectors, &cred_store, "notion").unwrap();
        let empty_list =
            list_connections_with_store(&conn_path, &connectors, &cred_store).unwrap();
        assert!(empty_list.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn endpoint_intersection_supports_wildcard_connection_with_matching_connector() {
        let notion = parse_and_validate_connector(BUILTIN_NOTION_CONNECTOR_JSON).unwrap();
        let other_conn = parse_and_validate_connector(
            r#"{
  "id": "other_service",
  "name": "Other",
  "official": false,
  "base_url": "https://api.other.example",
  "allowed_hosts": ["api.other.example"],
  "auth": { "type": "bearer", "credential_key": "token" },
  "test": { "method": "GET", "path": "/v1/users/me" },
  "endpoints": [{ "method": "GET", "path": "/v1/users/me" }]
}"#,
        )
        .unwrap();

        let wildcard_widget = parse_and_validate_widget_manifest(
            r#"{
  "id": "notion-spike",
  "name": "Notion Spike",
  "entry": "notion-spike.html",
  "connections": [
    {
      "connection": "*",
      "connector": "notion",
      "endpoints": [
        { "method": "GET", "path": "/v1/users/me" },
        { "method": "GET", "path": "/v1/databases/*" },
        { "method": "POST", "path": "/v1/data_sources/*/query" }
      ]
    },
    {
      "connection": "notion_limited",
      "connector": "notion",
      "endpoints": [
        { "method": "GET", "path": "/v1/users/me" }
      ]
    }
  ]
}"#,
        )
        .unwrap();

        // 任意接続名 "my-notion" は wildcard ("*", connector: "notion") に一致し、読み取り3種許可・PATCH拒否
        assert!(verify_endpoint_intersection(
            &notion,
            &wildcard_widget,
            "my-notion",
            "GET",
            "/v1/users/me"
        )
        .is_ok());
        assert!(verify_endpoint_intersection(
            &notion,
            &wildcard_widget,
            "my-notion",
            "GET",
            "/v1/databases/db-1"
        )
        .is_ok());
        assert!(verify_endpoint_intersection(
            &notion,
            &wildcard_widget,
            "my-notion",
            "PATCH",
            "/v1/pages/test"
        )
        .is_err());

        // 完全一致 "notion_limited" がある場合はワイルドカードより完全一致が優先される
        assert!(verify_endpoint_intersection(
            &notion,
            &wildcard_widget,
            "notion_limited",
            "GET",
            "/v1/users/me"
        )
        .is_ok());
        assert!(verify_endpoint_intersection(
            &notion,
            &wildcard_widget,
            "notion_limited",
            "GET",
            "/v1/databases/db-1"
        )
        .is_err());

        // connector が "notion" 以外の場合はワイルドカードに一致しない
        assert!(verify_endpoint_intersection(
            &other_conn,
            &wildcard_widget,
            "my-other",
            "GET",
            "/v1/users/me"
        )
        .is_err());
    }

    #[test]
    fn evaluate_test_response_formats_success_and_error_hints() {
        let ok_res = evaluate_test_response(
            "notion",
            "notion",
            200,
            r#"{"object":"user","id":"u1","name":"DesktopBot","type":"bot"}"#,
        );
        assert!(ok_res.ok);
        assert_eq!(ok_res.status, 200);
        assert!(ok_res.detail.contains("DesktopBot"));
        assert!(ok_res.hint.as_deref().unwrap().contains("コネクト"));

        let err_401 = evaluate_test_response(
            "notion",
            "notion",
            401,
            r#"{"object":"error","status":401,"code":"unauthorized","message":"API token is invalid."}"#,
        );
        assert!(!err_401.ok);
        assert_eq!(err_401.status, 401);
        assert!(err_401.detail.contains("unauthorized"));
        assert!(err_401.hint.as_deref().unwrap().contains("トークンが無効"));

        let err_404 = evaluate_test_response(
            "notion",
            "notion",
            404,
            r#"{"object":"error","status":404,"code":"object_not_found","message":"Could not find database."}"#,
        );
        assert!(!err_404.ok);
        assert!(err_404.hint.as_deref().unwrap().contains("共有（コネクト追加）"));

        let err_403 = evaluate_test_response(
            "notion",
            "notion",
            403,
            r#"{"object":"error","status":403,"code":"restricted_resource","message":"Insufficient permissions."}"#,
        );
        assert!(!err_403.ok);
        assert!(err_403.hint.as_deref().unwrap().contains("権限"));
    }

    #[test]
    fn widget_config_schema_validation_enforces_types_uniqueness_and_no_secrets() {
        // 正常: string と boolean のみ、デフォルト型一致
        let valid = parse_and_validate_widget_manifest(
            r#"{
  "id": "notion-today",
  "name": "Notion 今日のタスク",
  "config_schema": [
    { "key": "connection", "label": "接続 ID", "type": "string", "required": true, "default": "notion" },
    { "key": "show_done", "label": "完了表示", "type": "boolean", "required": false, "default": true }
  ]
}"#,
        );
        assert!(valid.is_ok());

        // 拒否: "secret" 型や未知型
        assert!(parse_and_validate_widget_manifest(
            r#"{"id":"w","name":"W","config_schema":[{"key":"api_token","label":"Token","type":"secret"}]}"#
        ).is_err());
        assert!(parse_and_validate_widget_manifest(
            r#"{"id":"w","name":"W","config_schema":[{"key":"count","label":"Count","type":"number"}]}"#
        ).is_err());

        // 拒否: キー重複
        assert!(parse_and_validate_widget_manifest(
            r#"{"id":"w","name":"W","config_schema":[{"key":"db","label":"A","type":"string"},{"key":"db","label":"B","type":"string"}]}"#
        ).is_err());

        // 拒否: 不正キー・空ラベル・不正デフォルト型
        assert!(parse_and_validate_widget_manifest(
            r#"{"id":"w","name":"W","config_schema":[{"key":"../evil","label":"DB","type":"string"}]}"#
        ).is_err());
        assert!(parse_and_validate_widget_manifest(
            r#"{"id":"w","name":"W","config_schema":[{"key":"db","label":"  ","type":"string"}]}"#
        ).is_err());
        assert!(parse_and_validate_widget_manifest(
            r#"{"id":"w","name":"W","config_schema":[{"key":"show_done","label":"完了表示","type":"boolean","default":"true"}]}"#
        ).is_err());
        assert!(parse_and_validate_widget_manifest(
            r#"{"id":"w","name":"W","config_schema":[{"key":"db","label":"DB","type":"string","default":false}]}"#
        ).is_err());
    }

    #[test]
    fn widget_config_read_save_defaults_empty_string_and_storage_isolation() {
        let dir = std::env::temp_dir().join(format!("dd_wcfg_test_{}", uuid::Uuid::new_v4()));
        let config_dir = dir.join("widget_config");
        let storage_dir = dir.join("storage");
        fs::create_dir_all(&dir).unwrap();

        let html_no_manifest = dir.join("plain.html");
        fs::write(&html_no_manifest, "<html></html>").unwrap();
        assert_eq!(
            read_effective_widget_config(&config_dir, &html_no_manifest, "w1").unwrap(),
            json!({})
        );

        let html_path = dir.join("notion-today.html");
        fs::write(&html_path, "<html></html>").unwrap();
        fs::write(
            dir.join("notion-today.manifest.json"),
            r#"{
  "id": "notion-today",
  "name": "Notion 今日のタスク",
  "entry": "notion-today.html",
  "config_schema": [
    { "key": "connection", "label": "接続 ID", "type": "string", "required": true, "default": "notion" },
    { "key": "database_id", "label": "DB ID", "type": "string", "required": true, "default": "" },
    { "key": "show_done", "label": "完了表示", "type": "boolean", "default": true },
    { "key": "show_overdue", "label": "期限切れ表示", "type": "boolean" }
  ]
}"#,
        )
        .unwrap();

        // 未保存時はデフォルト値補完（boolean で default 省略時は false）
        let initial = read_effective_widget_config(&config_dir, &html_path, "w1").unwrap();
        assert_eq!(
            initial,
            json!({
                "connection": "notion",
                "database_id": "",
                "show_done": true,
                "show_overdue": false
            })
        );

        // required: true の string に空文字 "" の保存を許可し、未知キーは除外する
        let mut raw = Map::new();
        raw.insert("connection".to_string(), json!("notion_work"));
        raw.insert("database_id".to_string(), json!(""));
        raw.insert("show_done".to_string(), json!(false));
        raw.insert("show_overdue".to_string(), json!(true));
        raw.insert("unknown_extra".to_string(), json!("should_be_stripped"));

        let saved_dto = save_widget_config_values(&config_dir, &html_path, "w1", &raw).unwrap();
        assert!(saved_dto.has_manifest);
        assert_eq!(saved_dto.manifest_id.as_deref(), Some("notion-today"));
        assert!(!saved_dto.values.contains_key("unknown_extra"));
        assert_eq!(saved_dto.values.get("database_id"), Some(&json!("")));

        // 型不一致は拒否
        let mut bad_type = Map::new();
        bad_type.insert("show_done".to_string(), json!("yes"));
        assert!(save_widget_config_values(&config_dir, &html_path, "w1", &bad_type).is_err());

        // DD.storage (storage/w1.json) とは完全分離される
        write_widget_storage_value(&storage_dir, "w1", "notion:data_source_id:db1", json!("ds-1"))
            .unwrap();
        assert!(config_dir.join("w1.json").is_file());
        assert!(storage_dir.join("w1.json").is_file());
        assert_eq!(
            read_widget_storage_value(&storage_dir, "w1", "connection").unwrap(),
            Value::Null
        );
        let reloaded = read_effective_widget_config(&config_dir, &html_path, "w1").unwrap();
        assert_eq!(reloaded.get("connection"), Some(&json!("notion_work")));
        assert_eq!(reloaded.get("show_done"), Some(&json!(false)));
        assert_eq!(reloaded.get("show_overdue"), Some(&json!(true)));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn validate_open_url_target_allows_http_https_and_rejects_dangerous_schemes() {
        assert_eq!(
            validate_open_url_target("https://www.notion.so/page-123")
                .unwrap()
                .scheme(),
            "https"
        );
        assert_eq!(
            validate_open_url_target("http://example.com/path")
                .unwrap()
                .scheme(),
            "http"
        );

        for bad in [
            "",
            "   ",
            "javascript:alert(1)",
            "file:///C:/Windows/win.ini",
            "data:text/html,<script>alert(1)</script>",
            "widgetfile://local/index.html",
            "http://",
            "https://example.com/\nnewline",
        ] {
            assert!(
                validate_open_url_target(bad).is_err(),
                "expected URL rejection for '{bad}'"
            );
        }
    }

    #[test]
    fn phase4c_capabilities_isolate_dd_and_manager_commands() {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let widgets_dd: Value = serde_json::from_str(
            &fs::read_to_string(manifest_dir.join("capabilities").join("widgets-dd.json")).unwrap(),
        )
        .unwrap();
        let default_cap: Value = serde_json::from_str(
            &fs::read_to_string(manifest_dir.join("capabilities").join("default.json")).unwrap(),
        )
        .unwrap();
        let widgets_cap: Value = serde_json::from_str(
            &fs::read_to_string(manifest_dir.join("capabilities").join("widgets.json")).unwrap(),
        )
        .unwrap();

        assert_eq!(widgets_dd.get("local"), Some(&json!(true)));
        assert!(widgets_dd.get("remote").is_none());
        let dd_perms = widgets_dd.get("permissions").and_then(Value::as_array).unwrap();
        for p in ["allow-dd-config-get", "allow-dd-open-url"] {
            assert!(
                dd_perms.iter().any(|v| v.as_str() == Some(p)),
                "widgets-dd.json missing {p}"
            );
        }

        let def_perms = default_cap.get("permissions").and_then(Value::as_array).unwrap();
        for p in ["allow-get-widget-config-form", "allow-save-widget-config"] {
            assert!(
                def_perms.iter().any(|v| v.as_str() == Some(p)),
                "default.json missing {p}"
            );
        }

        let w_perms = widgets_cap.get("permissions").and_then(Value::as_array).unwrap();
        assert!(
            !w_perms
                .iter()
                .filter_map(Value::as_str)
                .any(|p| p.starts_with("allow-dd-")),
            "widgets.json must not contain allow-dd-*"
        );
    }

    #[test]
    fn notion_today_manifest_intersection_allows_read_and_page_patch_and_top_level_connector() {
        let notion = parse_and_validate_connector(BUILTIN_NOTION_CONNECTOR_JSON).unwrap();
        let today_manifest = parse_and_validate_widget_manifest(
            r#"{
  "id": "notion-today",
  "name": "Notion 今日のタスク",
  "version": "0.2.0",
  "entry": "notion-today.html",
  "connections": [
    {
      "connection": "*",
      "connector": "notion",
      "endpoints": [
        { "method": "GET", "path": "/v1/databases/*" },
        { "method": "GET", "path": "/v1/data_sources/*" },
        { "method": "POST", "path": "/v1/data_sources/*/query" },
        { "method": "PATCH", "path": "/v1/pages/*" }
      ]
    }
  ]
}"#,
        )
        .unwrap();

        for (method, path) in [
            ("GET", "/v1/databases/db-123"),
            ("GET", "/v1/data_sources/ds-456"),
            ("POST", "/v1/data_sources/ds-456/query"),
            ("PATCH", "/v1/pages/page-789"),
        ] {
            assert!(
                verify_endpoint_intersection(&notion, &today_manifest, "notion", method, path)
                    .is_ok(),
                "expected {method} {path} to pass"
            );
        }

        // 未宣言の GET /v1/users/me や DELETE /v1/pages/page-789 は拒否
        assert!(
            verify_endpoint_intersection(&notion, &today_manifest, "notion", "GET", "/v1/users/me")
                .is_err()
        );
        assert!(verify_endpoint_intersection(
            &notion,
            &today_manifest,
            "notion",
            "DELETE",
            "/v1/pages/page-789"
        )
        .is_err());

        // トップレベル connector + endpoints 形式もサポートし、不一致コネクタは拒否
        let top_level_manifest = parse_and_validate_widget_manifest(
            r#"{
  "id": "top-level",
  "name": "Top Level",
  "connector": "notion",
  "endpoints": [
    { "method": "GET", "path": "/v1/databases/*" }
  ]
}"#,
        )
        .unwrap();
        assert!(verify_endpoint_intersection(
            &notion,
            &top_level_manifest,
            "my-notion",
            "GET",
            "/v1/databases/db-1"
        )
        .is_ok());

        let mut other_conn = notion.clone();
        other_conn.id = "other".to_string();
        assert!(verify_endpoint_intersection(
            &other_conn,
            &top_level_manifest,
            "my-other",
            "GET",
            "/v1/databases/db-1"
        )
        .is_err());
    }
}
