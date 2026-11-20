use crate::config::{ConfigState, PlacementMode, SourceType, WidgetConfig};
use crate::win32;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::webview::{NewWindowResponse, PageLoadEvent};
use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};

pub fn widget_label(widget_id: &str) -> String {
    format!("widget-{widget_id}")
}

/// ローカル HTML ガジェット (`SourceType::File`) のトップフレーム向け `localStorage` 名前空間シムを生成する。
fn build_local_storage_shim_script(widget_id: &str, allow_legacy_local_storage: bool) -> String {
    let widget_id_json = serde_json::to_string(widget_id).unwrap_or_else(|_| "\"\"".to_string());
    let allow_legacy_str = if allow_legacy_local_storage {
        "true"
    } else {
        "false"
    };
    let body = r#"
  if (window === window.top && location.origin === 'http://widgetfile.localhost' && !window.__ddLocalStorageShimmed) {
    let rawStorage = null;
    try { rawStorage = window.localStorage; } catch (_) {}
    if (rawStorage) {
      const rawProto = window.Storage && window.Storage.prototype ? window.Storage.prototype : rawStorage;
      const rawGet = (k) => rawProto.getItem.call(rawStorage, k);
      const rawSet = (k, v) => rawProto.setItem.call(rawStorage, k, v);
      const rawRemove = (k) => rawProto.removeItem.call(rawStorage, k);
      const rawKey = (i) => rawProto.key.call(rawStorage, i);
      const rawLen = () => rawStorage.length;

      const PREFIX = '__dd_w:' + WIDGET_ID + ':';
      const TOMBSTONE_KEY = '__dd_wt:' + WIDGET_ID;
      const CLEARED_KEY = '__dd_wc:' + WIDGET_ID;
      const isInternalKey = (k) =>
        k.startsWith('__dd_w:') || k.startsWith('__dd_wt:') || k.startsWith('__dd_wc:');
      const isLegacyCleared = () => ALLOW_LEGACY_FALLBACK && rawGet(CLEARED_KEY) === '1';

      const readTombstones = () => {
        if (!ALLOW_LEGACY_FALLBACK) return new Set();
        try {
          const raw = rawGet(TOMBSTONE_KEY);
          if (!raw) return new Set();
          const arr = JSON.parse(raw);
          return Array.isArray(arr) ? new Set(arr.filter((x) => typeof x === 'string')) : new Set();
        } catch (_) {
          return new Set();
        }
      };

      const writeTombstones = (set) => {
        if (!ALLOW_LEGACY_FALLBACK) return;
        try {
          if (set.size === 0) {
            rawRemove(TOMBSTONE_KEY);
          } else {
            rawSet(TOMBSTONE_KEY, JSON.stringify(Array.from(set)));
          }
        } catch (_) {}
      };

      const peekItem = (key) => {
        const k = String(key);
        const ownVal = rawGet(PREFIX + k);
        if (ownVal !== null) return ownVal;
        if (!ALLOW_LEGACY_FALLBACK || isLegacyCleared() || isInternalKey(k)) {
          return null;
        }
        if (readTombstones().has(k)) {
          return null;
        }
        return rawGet(k);
      };

      const getItem = (key) => {
        const k = String(key);
        const ownVal = rawGet(PREFIX + k);
        if (ownVal !== null) return ownVal;
        if (!ALLOW_LEGACY_FALLBACK || isLegacyCleared() || isInternalKey(k)) {
          return null;
        }
        if (readTombstones().has(k)) {
          return null;
        }
        const legacyVal = rawGet(k);
        if (legacyVal !== null) {
          try {
            rawSet(PREFIX + k, legacyVal);
          } catch (_) {}
        }
        return legacyVal;
      };

      const setItem = (key, value) => {
        const k = String(key);
        const v = String(value);
        rawSet(PREFIX + k, v);
        if (ALLOW_LEGACY_FALLBACK) {
          const tomb = readTombstones();
          if (tomb.delete(k)) {
            writeTombstones(tomb);
          }
        }
      };

      const removeItem = (key) => {
        const k = String(key);
        rawRemove(PREFIX + k);
        if (ALLOW_LEGACY_FALLBACK && !isLegacyCleared() && !isInternalKey(k)) {
          if (rawGet(k) !== null) {
            const tomb = readTombstones();
            tomb.add(k);
            writeTombstones(tomb);
          }
        }
      };

      const clear = () => {
        const toRemove = [];
        const total = rawLen();
        for (let i = 0; i < total; i += 1) {
          const rk = rawKey(i);
          if (typeof rk === 'string' && rk.startsWith(PREFIX)) {
            toRemove.push(rk);
          }
        }
        for (const rk of toRemove) {
          rawRemove(rk);
        }
        if (ALLOW_LEGACY_FALLBACK) {
          rawSet(CLEARED_KEY, '1');
          rawRemove(TOMBSTONE_KEY);
        }
      };

      const listLogicalKeys = () => {
        const seen = new Set();
        const result = [];
        const tomb = ALLOW_LEGACY_FALLBACK && !isLegacyCleared() ? readTombstones() : null;
        const total = rawLen();
        for (let i = 0; i < total; i += 1) {
          const rk = rawKey(i);
          if (typeof rk !== 'string') continue;
          if (rk.startsWith(PREFIX)) {
            const logical = rk.slice(PREFIX.length);
            if (!seen.has(logical)) {
              seen.add(logical);
              result.push(logical);
            }
          } else if (
            ALLOW_LEGACY_FALLBACK &&
            !isLegacyCleared() &&
            !isInternalKey(rk) &&
            !(tomb && tomb.has(rk))
          ) {
            if (!seen.has(rk)) {
              seen.add(rk);
              result.push(rk);
            }
          }
        }
        return result;
      };

      const api = {
        getItem: (k) => getItem(k),
        setItem: (k, v) => setItem(k, v),
        removeItem: (k) => removeItem(k),
        clear: () => clear(),
        key: (idx) => {
          const keys = listLogicalKeys();
          const i = Number(idx) >>> 0;
          return i < keys.length ? keys[i] : null;
        },
      };

      const lsProxy = new Proxy(api, {
        get(target, prop) {
          if (prop === 'length') return listLogicalKeys().length;
          if (Object.prototype.hasOwnProperty.call(target, prop)) {
            return target[prop];
          }
          if (typeof prop === 'symbol') {
            return Reflect.get(target, prop);
          }
          const val = peekItem(prop);
          if (val !== null) return val;
          if (prop in Object.prototype) {
            return Reflect.get(Object.prototype, prop);
          }
          return undefined;
        },
        set(target, prop, value) {
          if (typeof prop === 'symbol') return false;
          if (prop === 'length' || Object.prototype.hasOwnProperty.call(target, prop)) {
            return true;
          }
          setItem(prop, value);
          return true;
        },
        deleteProperty(_target, prop) {
          if (typeof prop === 'symbol') return false;
          removeItem(prop);
          return true;
        },
        has(target, prop) {
          if (typeof prop === 'symbol') return false;
          if (prop === 'length' || Object.prototype.hasOwnProperty.call(target, prop)) return true;
          return peekItem(prop) !== null;
        },
        ownKeys() {
          return listLogicalKeys();
        },
        getOwnPropertyDescriptor(_target, prop) {
          if (typeof prop !== 'string') return undefined;
          const keys = listLogicalKeys();
          if (!keys.includes(prop)) return undefined;
          const val = peekItem(prop);
          return {
            enumerable: true,
            configurable: true,
            writable: true,
            value: val === null ? '' : val,
          };
        },
      });

      try {
        Object.defineProperty(window, 'localStorage', {
          get() { return lsProxy; },
          configurable: false,
          enumerable: true,
        });
        window.__ddLocalStorageShimmed = true;
        window.__ddClearOwnLocalStorage = () => {
          const toRemove = [];
          const total = rawLen();
          for (let i = 0; i < total; i += 1) {
            const rk = rawKey(i);
            if (
              typeof rk === 'string' &&
              (rk.startsWith(PREFIX) || rk === TOMBSTONE_KEY || rk === CLEARED_KEY)
            ) {
              toRemove.push(rk);
            }
          }
          for (const rk of toRemove) {
            rawRemove(rk);
          }
        };
      } catch (_) {}
    }
  }
"#;
    format!(
        "\n  (() => {{\n    const WIDGET_ID = {widget_id_json};\n    const ALLOW_LEGACY_FALLBACK = {allow_legacy_str};{body}  }})();\n"
    )
}

/// ガジェット用 WebView ランタイムスクリプト（ホバー操作バー + リロードガード + カスタムCSS + Fileガジェット向けDDブリッジ/localStorage分離）を生成する。
fn build_widget_init_script(
    widget_id: &str,
    locked: bool,
    home_url: &str,
    custom_css: &str,
    inject_dd_bridge: bool,
    allow_legacy_local_storage: bool,
    duplicable: bool,
) -> String {
    let home_json = serde_json::to_string(home_url).unwrap_or_else(|_| "\"\"".to_string());
    let css_json = serde_json::to_string(custom_css).unwrap_or_else(|_| "\"\"".to_string());
    let locked_str = if locked { "true" } else { "false" };
    let duplicate_btn_script = if inject_dd_bridge && duplicable {
        "    btnGroup.appendChild(mkBtn('＋', '新しいウィンドウを増やす', () => invokeAction('duplicate')));\n"
    } else {
        ""
    };
    let local_storage_shim_script = if inject_dd_bridge {
        build_local_storage_shim_script(widget_id, allow_legacy_local_storage)
    } else {
        String::new()
    };
    let dd_bridge_script = if inject_dd_bridge {
        r#"
  if (window === window.top && location.origin === 'http://widgetfile.localhost' && !window.DD) {
    let inFlightMutations = 0;
    __ddHasActiveMutations = () => inFlightMutations > 0;
    const invokeBridge = (cmd, payload) => {
      if (!window.__TAURI_INTERNALS__ || typeof window.__TAURI_INTERNALS__.invoke !== 'function') {
        return Promise.reject(new Error('Tauri internals unavailable'));
      }
      return window.__TAURI_INTERNALS__.invoke(cmd, payload);
    };
    const storage = Object.freeze({
      get(key) {
        return invokeBridge('dd_storage_get', { key: String(key) });
      },
      set(key, value) {
        return invokeBridge('dd_storage_set', {
          key: String(key),
          value: value === undefined ? null : value,
        });
      },
    });
    const connect = Object.freeze({
      async request(opts) {
        const { connection, method, path, body } = opts || {};
        const normMethod = String(method || 'GET').trim().toUpperCase();
        const isMutation = normMethod !== 'GET' && normMethod !== 'HEAD';
        if (isMutation) {
          inFlightMutations += 1;
        }
        try {
          return await invokeBridge('dd_connect_request', {
            request: {
              connection: String(connection || ''),
              method: normMethod,
              path: String(path || ''),
              body: body === undefined ? null : body,
            },
          });
        } finally {
          if (isMutation) {
            inFlightMutations = Math.max(0, inFlightMutations - 1);
            markInteraction();
          }
        }
      },
    });
    const config = Object.freeze({
      get() {
        return invokeBridge('dd_config_get', {});
      },
    });
    const openUrl = (url) => {
      return invokeBridge('dd_open_url', { url: String(url || '') });
    };
    Object.defineProperty(window, 'DD', {
      value: Object.freeze({ storage, connect, config, openUrl }),
      writable: false,
      configurable: false,
      enumerable: true,
    });
  }
"#
    } else {
        ""
    };

    format!(
        r#"
(() => {{
  if (window !== window.top) return;
  if (window.__ddRuntimeInstalled) return;
  window.__ddRuntimeInstalled = true;

  const HOME_URL = {home_json};
  let isLocked = {locked_str};
  let currentCustomCss = {css_json};
  let lastInteractionAt = 0;
  let pendingReloadTimer = null;
  let hostEl = null;
  let styleEl = null;
  let __ddHasActiveMutations = null;

  const markInteraction = () => {{ lastInteractionAt = Date.now(); }};
  document.addEventListener('pointerdown', markInteraction, true);
  document.addEventListener('keydown', markInteraction, true);
  document.addEventListener('input', markInteraction, true);
  document.addEventListener('focusin', markInteraction, true);
{local_storage_shim_script}{dd_bridge_script}
  function isUserEditingOrActive() {{
    if (typeof __ddHasActiveMutations === 'function' && __ddHasActiveMutations()) {{
      return true;
    }}
    const active = document.activeElement;
    if (active) {{
      const tag = active.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || active.isContentEditable) {{
        return true;
      }}
    }}
    return (Date.now() - lastInteractionAt) < 5000;
  }}

  window.__ddSafeReload = () => {{
    if (isUserEditingOrActive()) {{
      if (!pendingReloadTimer) {{
        pendingReloadTimer = setTimeout(() => {{
          pendingReloadTimer = null;
          window.__ddSafeReload();
        }}, 1500);
      }}
      return;
    }}
    location.reload();
  }};

  window.__ddSetLocked = (nextLocked) => {{
    isLocked = !!nextLocked;
    if (hostEl) {{
      hostEl.style.display = isLocked ? 'none' : 'block';
    }}
  }};

  window.__ddApplyCustomCss = (css) => {{
    currentCustomCss = typeof css === 'string' ? css : '';
    const target = document.head || document.documentElement || document.body;
    if (!target) return;
    let existingStyle = document.getElementById('__dd_custom_css');
    if (!existingStyle) {{
      existingStyle = document.createElement('style');
      existingStyle.id = '__dd_custom_css';
      target.appendChild(existingStyle);
    }}
    styleEl = existingStyle;
    styleEl.textContent = currentCustomCss;
  }};

  function invokeAction(action) {{
    if (window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function') {{
      window.__TAURI_INTERNALS__.invoke('widget_bar_action', {{ action }}).catch(() => {{}});
    }}
  }}

  function install() {{
    window.__ddApplyCustomCss(currentCustomCss);
    const parent = document.documentElement || document.body;
    if (!parent) return;
    const existing = document.getElementById('__dd_drag_host');
    if (existing) {{
      hostEl = existing;
      hostEl.style.display = isLocked ? 'none' : 'block';
      return;
    }}
    hostEl = document.createElement('div');
    hostEl.id = '__dd_drag_host';
    hostEl.style.cssText = 'all:initial;position:fixed;top:0;left:0;right:0;height:28px;z-index:2147483647;pointer-events:auto;';
    hostEl.style.display = isLocked ? 'none' : 'block';

    const shadow = hostEl.attachShadow({{ mode: 'open' }});
    const bar = document.createElement('div');
    bar.style.cssText = [
      'width:100%',
      'height:28px',
      'display:flex',
      'align-items:center',
      'justify-content:space-between',
      'padding:4px 8px 2px 8px',
      'box-sizing:border-box',
      'font-family:"Segoe UI",sans-serif',
      'font-size:11px',
      'color:rgba(255,255,255,0.92)',
      'background:rgba(15,23,42,0.82)',
      'user-select:none',
      'opacity:0',
      'transition:opacity 0.15s ease'
    ].join(';');

    const dragArea = document.createElement('div');
    dragArea.textContent = '⋮⋮ ドラッグで移動';
    dragArea.style.cssText = 'flex:1;cursor:move;display:flex;align-items:center;height:100%;';
    dragArea.addEventListener('mousedown', (e) => {{
      if (e.button !== 0 || isLocked) return;
      e.preventDefault();
      if (window.__TAURI_INTERNALS__ && typeof window.__TAURI_INTERNALS__.invoke === 'function') {{
        window.__TAURI_INTERNALS__.invoke('plugin:window|start_dragging').catch(() => {{}});
      }}
    }});

    const btnGroup = document.createElement('div');
    btnGroup.style.cssText = 'display:flex;align-items:center;gap:4px;pointer-events:auto;';

    const mkBtn = (label, title, onClick) => {{
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = label;
      b.title = title;
      b.style.cssText = 'border:none;background:rgba(255,255,255,0.16);color:#fff;border-radius:4px;cursor:pointer;font-size:12px;line-height:1;min-width:24px;height:20px;padding:0 6px;display:inline-flex;align-items:center;justify-content:center;';
      b.addEventListener('mousedown', (e) => e.stopPropagation());
      b.addEventListener('click', (e) => {{ e.preventDefault(); e.stopPropagation(); onClick(); }});
      return b;
    }};

{duplicate_btn_script}    btnGroup.appendChild(mkBtn('⌂', 'ホームへ戻る', () => invokeAction('home')));
    btnGroup.appendChild(mkBtn('↻', '再読み込み', () => invokeAction('reload')));
    btnGroup.appendChild(mkBtn('🔒', 'ロックする', () => invokeAction('lock')));
    btnGroup.appendChild(mkBtn('✕', '閉じる（非表示）', () => invokeAction('close')));

    bar.appendChild(dragArea);
    bar.appendChild(btnGroup);

    bar.addEventListener('mouseenter', () => {{ if (!isLocked) bar.style.opacity = '1'; }});
    bar.addEventListener('mouseleave', () => {{ bar.style.opacity = '0'; }});

    shadow.appendChild(bar);
    parent.appendChild(hostEl);
  }}

  install();
  if (document.readyState === 'loading') {{
    document.addEventListener('DOMContentLoaded', install, {{ once: true }});
  }}
  window.addEventListener('load', install);

  try {{
    const obs = new MutationObserver(() => {{
      if (!document.getElementById('__dd_drag_host')) {{
        install();
      }} else if (currentCustomCss && !document.getElementById('__dd_custom_css')) {{
        window.__ddApplyCustomCss(currentCustomCss);
      }}
    }});
    if (document.documentElement) {{
      obs.observe(document.documentElement, {{ childList: true, subtree: true }});
    }}
  }} catch (_) {{}}
}})();
"#
    )
}

/// 独立した WebView2 `user_data_dir` を解決する（他アプリとのロック競合防止）。
pub fn resolve_webview_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let base = app
        .path()
        .local_data_dir()
        .map_err(|e| format!("failed to resolve local_data_dir: {e}"))?;
    let dir = base.join("DesktopDashboards").join("webview");
    fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create webview data dir {}: {e}", dir.display()))?;
    Ok(dir)
}

/// ガジェット設定から WebView の初期 URL を構築する。
pub fn build_webview_url(widget: &WidgetConfig) -> Result<WebviewUrl, String> {
    match widget.source_type {
        SourceType::Url => {
            let parsed = url::Url::parse(&widget.source)
                .map_err(|e| format!("invalid URL '{}': {e}", widget.source))?;
            Ok(WebviewUrl::External(parsed))
        }
        SourceType::File => {
            let parsed = url::Url::parse(&format!("http://widgetfile.localhost/{}/", widget.id))
                .map_err(|e| format!("failed to build widgetfile URL: {e}"))?;
            Ok(WebviewUrl::External(parsed))
        }
    }
}

/// 遷移先 URL が初期 URL と同一オリジンまたは内部スキームかを判定する。
pub fn is_same_origin_or_internal(initial: &url::Url, target: &url::Url) -> bool {
    if matches!(target.scheme(), "about" | "data" | "blob") {
        return true;
    }
    initial.scheme() == target.scheme()
        && initial.host_str() == target.host_str()
        && initial.port_or_known_default() == target.port_or_known_default()
}

/// ガジェット設定に応じて配置モード（モードA: 最背面固定 / モードB: WorkerW 壁紙埋め込み）を適用する。
pub fn apply_placement_mode(window: &tauri::WebviewWindow, widget: &WidgetConfig) {
    let w = widget.width.max(120);
    let h = widget.height.max(80);
    match widget.placement_mode {
        PlacementMode::Bottom => {
            if let Err(e) = win32::apply_bottom_mode_with_pos(window, widget.x, widget.y, w, h) {
                eprintln!("[widget] apply_bottom_mode failed: {e}");
            }
        }
        PlacementMode::WorkerW => {
            if let Err(e) = win32::apply_workerw_mode(window, widget.x, widget.y, w, h) {
                eprintln!("[widget] apply_workerw_mode failed (fallback to Bottom): {e}");
                let _ = win32::apply_bottom_mode_with_pos(window, widget.x, widget.y, w, h);
            }
        }
    }
}

/// 既存ガジェットウィンドウにランタイム設定（配置モード・ロック・透明度・クリック透過・NOACTIVATE・ズーム・カスタムCSS）を適用する。
pub fn apply_runtime_settings(app: &AppHandle, widget: &WidgetConfig) {
    let label = widget_label(&widget.id);
    let Some(window) = app.get_webview_window(&label) else {
        return;
    };

    apply_placement_mode(&window, widget);

    let _ = window.set_resizable(!widget.locked);
    let _ = window.set_zoom(widget.zoom.clamp(0.5, 2.0));
    let _ = window.eval(&format!(
        "window.__ddSetLocked && window.__ddSetLocked({});",
        if widget.locked { "true" } else { "false" }
    ));
    if let Ok(css_json) = serde_json::to_string(&widget.custom_css) {
        let _ = window.eval(&format!(
            "window.__ddApplyCustomCss && window.__ddApplyCustomCss({css_json});"
        ));
    }

    if let Err(e) = win32::apply_widget_styles(
        &window,
        widget.opacity,
        widget.click_through,
        widget.no_activate,
    ) {
        eprintln!("[widget] failed to apply styles to {label}: {e}");
    }
}

/// ガジェットウィンドウを生成する（コマンド内からの呼び出し時のデッドロック防止のため async fn）。
pub async fn create_widget_window(app: AppHandle, widget: WidgetConfig) -> Result<(), String> {
    let label = widget_label(&widget.id);
    if app.get_webview_window(&label).is_some() {
        apply_runtime_settings(&app, &widget);
        return Ok(());
    }

    let webview_url = build_webview_url(&widget)?;
    let initial_url = match &webview_url {
        WebviewUrl::External(u) => Some(u.clone()),
        _ => None,
    };
    let home_url_str = initial_url
        .as_ref()
        .map(|u| u.to_string())
        .unwrap_or_default();
    let data_dir = resolve_webview_data_dir(&app)?;

    let scale = app
        .primary_monitor()
        .ok()
        .flatten()
        .map(|m| m.scale_factor())
        .unwrap_or(1.0)
        .max(1.0);
    let target_w = widget.width.max(120);
    let target_h = widget.height.max(80);
    let inject_dd_bridge = widget.source_type == SourceType::File;
    let allow_legacy_local_storage = inject_dd_bridge && !widget.local_storage_isolated;
    let duplicable = if inject_dd_bridge {
        crate::dd_bridge::is_widget_duplicable(&app, &widget)
    } else {
        false
    };
    let init_script = build_widget_init_script(
        &widget.id,
        widget.locked,
        &home_url_str,
        &widget.custom_css,
        inject_dd_bridge,
        allow_legacy_local_storage,
        duplicable,
    );

    let mut builder = WebviewWindowBuilder::new(&app, &label, webview_url)
        .title(&widget.title)
        .data_directory(data_dir)
        .position(widget.x as f64 / scale, widget.y as f64 / scale)
        .inner_size(target_w as f64 / scale, target_h as f64 / scale)
        .decorations(false)
        .shadow(false)
        .resizable(!widget.locked)
        .skip_taskbar(true)
        .always_on_bottom(true)
        .initialization_script(&init_script)
        .on_page_load({
            let widget_id = widget.id.clone();
            move |win, payload| {
                if payload.event() == PageLoadEvent::Finished {
                    let app_handle = win.app_handle();
                    if let Some(state) = app_handle.try_state::<Arc<ConfigState>>() {
                        if let Ok(cfg) = state.config.lock() {
                            if let Some(w) = cfg.widgets.iter().find(|w| w.id == widget_id) {
                                let _ = win.set_zoom(w.zoom.clamp(0.5, 2.0));
                                let _ = win.eval(&format!(
                                    "window.__ddSetLocked && window.__ddSetLocked({});",
                                    if w.locked { "true" } else { "false" }
                                ));
                                if let Ok(css_json) = serde_json::to_string(&w.custom_css) {
                                    let _ = win.eval(&format!(
                                        "window.__ddApplyCustomCss && window.__ddApplyCustomCss({css_json});"
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        })
        .on_new_window(|url, _features| {
            if matches!(url.scheme(), "http" | "https") {
                let _ = tauri_plugin_opener::open_url(url.as_str(), None::<&str>);
            }
            NewWindowResponse::Deny
        });

    if let Some(base_url) = initial_url {
        builder = builder.on_navigation(move |target_url| {
            if is_same_origin_or_internal(&base_url, target_url) {
                true
            } else {
                if matches!(target_url.scheme(), "http" | "https") {
                    let _ = tauri_plugin_opener::open_url(target_url.as_str(), None::<&str>);
                }
                false
            }
        });
    }

    let window = builder
        .build()
        .map_err(|e| format!("failed to create widget window {label}: {e}"))?;

    // 物理ピクセル座標で厳密に配置し、Moved イベントによる座標ズレを防ぐ
    let _ = window.set_position(Position::Physical(PhysicalPosition::new(
        widget.x, widget.y,
    )));
    let _ = window.set_size(Size::Physical(PhysicalSize::new(target_w, target_h)));

    // 配置モード（モードA: 最背面固定 / モードB: WorkerW 埋め込み）と Win32 拡張スタイル適用
    apply_placement_mode(&window, &widget);
    if let Err(e) = win32::apply_widget_styles(
        &window,
        widget.opacity,
        widget.click_through,
        widget.no_activate,
    ) {
        eprintln!("[widget] failed to apply styles to {label}: {e}");
    }
    let _ = window.set_zoom(widget.zoom.clamp(0.5, 2.0));

    // 位置・サイズ変更時のデバウンス保存（WorkerW 子ウィンドウでも仮想スクリーン座標を保存）
    let app_handle = app.clone();
    let widget_id = widget.id.clone();
    let win_for_events = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Moved(_) | WindowEvent::Resized(_) => {
            let Some((sx, sy, sw, sh)) = win32::get_window_screen_rect(&win_for_events) else {
                return;
            };
            if sx <= -16000 || sy <= -16000 || sw == 0 || sh == 0 {
                return;
            }
            if let Some(state) = app_handle.try_state::<Arc<ConfigState>>() {
                if let Ok(mut cfg) = state.config.lock() {
                    if let Some(w) = cfg.widgets.iter_mut().find(|w| w.id == widget_id) {
                        if w.locked {
                            return;
                        }
                        w.x = sx;
                        w.y = sy;
                        w.width = sw;
                        w.height = sh;
                    }
                }
                ConfigState::schedule_debounced_save(state.inner());
            }
        }
        _ => {}
    });

    Ok(())
}

/// ガジェットウィンドウを破棄（close）してメモリを解放する。
pub fn close_widget_window(app: &AppHandle, widget_id: &str) {
    let label = widget_label(widget_id);
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.close();
    }
}

/// ガジェット専用の `localStorage` 名前空間をクリアした上でウィンドウを破棄（close）する。
pub async fn clear_storage_and_close_widget_window(app: &AppHandle, widget_id: &str) {
    let label = widget_label(widget_id);
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.eval("window.__ddClearOwnLocalStorage && window.__ddClearOwnLocalStorage();");
        let _ = tauri::async_runtime::spawn_blocking(|| {
            std::thread::sleep(Duration::from_millis(60));
        })
        .await;
        let _ = win.close();
    }
}

/// 指定ガジェットウィンドウを再読み込みする。
pub fn reload_widget_window(app: &AppHandle, widget_id: &str) {
    let label = widget_label(widget_id);
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.reload();
    }
}

/// 指定ガジェットウィンドウをリロードガード付きで再読み込みする。
pub fn safe_reload_widget_window(app: &AppHandle, widget_id: &str) {
    let label = widget_label(widget_id);
    if let Some(win) = app.get_webview_window(&label) {
        let _ = win.eval("window.__ddSafeReload ? window.__ddSafeReload() : location.reload();");
    }
}

/// 表示中の全ガジェットウィンドウをリロードガード付きで再読み込みする。
pub fn reload_all_widget_windows(app: &AppHandle) {
    for (label, win) in app.webview_windows() {
        if label.starts_with("widget-") {
            let _ =
                win.eval("window.__ddSafeReload ? window.__ddSafeReload() : location.reload();");
        }
    }
}

/// モード B（WorkerW 埋め込み）の全ガジェットを再アタッチする。
pub fn reattach_workerw_widgets(app: &AppHandle) {
    let widgets = {
        let Some(state) = app.try_state::<Arc<ConfigState>>() else {
            return;
        };
        let Ok(cfg) = state.config.lock() else {
            return;
        };
        cfg.widgets.clone()
    };

    for w in widgets {
        if w.enabled && w.placement_mode == PlacementMode::WorkerW {
            let label = widget_label(&w.id);
            if let Some(win) = app.get_webview_window(&label) {
                if !win32::is_attached_to_current_workerw(&win) {
                    apply_placement_mode(&win, &w);
                    let _ = win32::apply_widget_styles(
                        &win,
                        w.opacity,
                        w.click_through,
                        w.no_activate,
                    );
                }
            }
        }
    }
}

/// Explorer 再起動（TaskbarCreated）および壁紙変更による WorkerW 再生成を監視し、自動再アタッチする。
pub fn start_workerw_watcher(app: AppHandle) {
    let app_for_msg = app.clone();
    win32::start_taskbar_created_listener(move || {
        // Explorer 再起動直後は WorkerW 構築に少し時間がかかる場合があるため、少し待ってから再アタッチ
        let app_inner = app_for_msg.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(800));
            reattach_workerw_widgets(&app_inner);
        });
    });

    // 壁紙スライドショー切替や Win11 での WorkerW 再生成向けの定期生存監視（3秒ごと）
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        reattach_workerw_widgets(&app);
    });
}

/// 定期自動リロードループを起動する（30秒ごとに経過間隔をチェック）。
pub fn start_auto_reload_loop(app: AppHandle) {
    std::thread::spawn(move || {
        let mut last_reloaded: HashMap<String, Instant> = HashMap::new();
        loop {
            std::thread::sleep(Duration::from_secs(30));
            let widgets = {
                let Some(state) = app.try_state::<Arc<ConfigState>>() else {
                    continue;
                };
                let Ok(cfg) = state.config.lock() else {
                    continue;
                };
                cfg.widgets.clone()
            };

            let now = Instant::now();
            for w in widgets {
                if !w.enabled || w.auto_reload_minutes == 0 {
                    last_reloaded.remove(&w.id);
                    continue;
                }
                let interval = Duration::from_secs(u64::from(w.auto_reload_minutes) * 60);
                let prev = last_reloaded.entry(w.id.clone()).or_insert(now);
                if now.duration_since(*prev) >= interval {
                    *prev = now;
                    safe_reload_widget_window(&app, &w.id);
                }
            }
        }
    });
}

/// 設定されている全ガジェットの有効状態に合わせてウィンドウを同期する。
pub async fn sync_all_widgets(app: AppHandle) {
    let widgets = {
        let Some(state) = app.try_state::<Arc<ConfigState>>() else {
            return;
        };
        let Ok(cfg) = state.config.lock() else {
            return;
        };
        cfg.widgets.clone()
    };

    for widget in widgets {
        if widget.enabled {
            if let Err(e) = create_widget_window(app.clone(), widget).await {
                eprintln!("[widget] failed to restore widget: {e}");
            }
        } else {
            close_widget_window(&app, &widget.id);
        }
    }
}

/// ローカル HTML (`source_type: "file"`) 用カスタムプロトコル `widgetfile` のハンドラ。
pub fn handle_widgetfile_protocol(
    app: &AppHandle,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    let path = request.uri().path();
    let trimmed = path.trim_start_matches('/');
    let mut parts = trimmed.splitn(2, '/');
    let widget_id = parts.next().unwrap_or("");
    let rel_path = parts.next().unwrap_or("");

    if widget_id.is_empty() {
        return text_response(400, "missing widget id");
    }

    let source_path = {
        let Some(state) = app.try_state::<Arc<ConfigState>>() else {
            return text_response(500, "config state unavailable");
        };
        let Ok(cfg) = state.config.lock() else {
            return text_response(500, "config lock poisoned");
        };
        cfg.widgets
            .iter()
            .find(|w| w.id == widget_id && w.source_type == SourceType::File)
            .map(|w| PathBuf::from(&w.source))
    };

    let Some(mut entry_file) = source_path else {
        return text_response(404, "widget not found");
    };

    if !entry_file.is_file() {
        if let Some(file_name) = entry_file.file_name() {
            if let Some(samples_dir) = crate::resolve_samples_dir(app) {
                let fallback = samples_dir.join(file_name);
                if fallback.is_file() {
                    entry_file = fallback;
                }
            }
        }
    }

    let target_file = if rel_path.is_empty() {
        entry_file.clone()
    } else {
        let Some(base_dir) = entry_file.parent() else {
            return text_response(404, "invalid base directory");
        };
        let candidate = base_dir.join(rel_path);
        match (fs::canonicalize(base_dir), fs::canonicalize(&candidate)) {
            (Ok(canon_base), Ok(canon_target)) if canon_target.starts_with(&canon_base) => {
                canon_target
            }
            _ => return text_response(403, "forbidden path"),
        }
    };

    match fs::read(&target_file) {
        Ok(bytes) => {
            let mime = guess_mime_type(&target_file);
            tauri::http::Response::builder()
                .status(200)
                .header("Content-Type", mime)
                .body(bytes)
                .unwrap_or_else(|_| text_response(500, "response build error"))
        }
        Err(_) => text_response(404, "file not found"),
    }
}

fn text_response(status: u16, msg: &str) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(msg.as_bytes().to_vec())
        .unwrap()
}

fn guess_mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js") | Some("mjs") => "application/javascript; charset=utf-8",
        Some("json") => "application/json; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_same_origin_and_external() {
        let base = url::Url::parse("https://muse.ai/library/artifacts/abc").unwrap();
        let same = url::Url::parse("https://muse.ai/spaces/xyz").unwrap();
        let diff_host = url::Url::parse("https://example.com/page").unwrap();
        let diff_scheme = url::Url::parse("http://muse.ai/page").unwrap();
        let about = url::Url::parse("about:blank").unwrap();

        assert!(is_same_origin_or_internal(&base, &same));
        assert!(!is_same_origin_or_internal(&base, &diff_host));
        assert!(!is_same_origin_or_internal(&base, &diff_scheme));
        assert!(is_same_origin_or_internal(&base, &about));
    }

    #[test]
    fn init_script_injects_dd_bridge_only_for_file_widgets() {
        let file_script = build_widget_init_script(
            "w1",
            false,
            "http://widgetfile.localhost/w1/",
            "",
            true,
            false,
            false,
        );
        assert!(file_script.contains("if (window !== window.top) return;"));
        assert!(file_script.contains(
            "window === window.top && location.origin === 'http://widgetfile.localhost' && !window.DD"
        ));
        assert!(file_script.contains("Object.defineProperty(window, 'DD'"));
        assert!(file_script.contains("Object.freeze({ storage, connect, config, openUrl })"));
        assert!(file_script.contains("dd_storage_get"));
        assert!(file_script.contains("dd_storage_set"));
        assert!(file_script.contains("dd_connect_request"));
        assert!(file_script.contains("dd_config_get"));
        assert!(file_script.contains("dd_open_url"));
        assert!(file_script.contains("inFlightMutations"));

        let url_script = build_widget_init_script(
            "w-url",
            false,
            "https://example.com/",
            "",
            false,
            false,
            false,
        );
        assert!(url_script.contains("if (window !== window.top) return;"));
        assert!(!url_script.contains("Object.defineProperty(window, 'DD'"));
        assert!(!url_script.contains("dd_storage_get"));
        assert!(!url_script.contains("dd_storage_set"));
        assert!(!url_script.contains("dd_connect_request"));
        assert!(!url_script.contains("dd_config_get"));
        assert!(!url_script.contains("dd_open_url"));
        assert!(!url_script.contains("inFlightMutations"));
        assert!(!url_script.contains("__ddLocalStorageShimmed"));
        assert!(!url_script.contains("invokeAction('duplicate')"));
    }

    #[test]
    fn init_script_local_storage_shim_and_duplicate_button() {
        let legacy_dup = build_widget_init_script(
            "legacy-1",
            false,
            "http://widgetfile.localhost/legacy-1/",
            "",
            true,
            true,
            true,
        );
        assert!(legacy_dup.contains(r#"const WIDGET_ID = "legacy-1";"#));
        assert!(legacy_dup.contains("const ALLOW_LEGACY_FALLBACK = true;"));
        assert!(legacy_dup.contains("__ddLocalStorageShimmed"));
        assert!(legacy_dup.contains("__ddClearOwnLocalStorage"));
        assert!(legacy_dup.contains("invokeAction('duplicate')"));
        assert!(legacy_dup.contains("新しいウィンドウを増やす"));

        let isolated_non_dup = build_widget_init_script(
            "dup-2",
            false,
            "http://widgetfile.localhost/dup-2/",
            "",
            true,
            false,
            false,
        );
        assert!(isolated_non_dup.contains(r#"const WIDGET_ID = "dup-2";"#));
        assert!(isolated_non_dup.contains("const ALLOW_LEGACY_FALLBACK = false;"));
        assert!(isolated_non_dup.contains("__ddLocalStorageShimmed"));
        assert!(!isolated_non_dup.contains("invokeAction('duplicate')"));
    }
}
