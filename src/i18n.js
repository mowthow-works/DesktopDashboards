(() => {
  const TRANSLATIONS = {
    ja: {
      docTitle: "DesktopDashboards - 管理画面",
      langLabel: "言語",
      langAuto: "自動 (OS設定)",
      langJa: "日本語",
      langEn: "English",
      langUpdated: "表示言語を日本語に切り替えました。",
      reloadAll: "すべて再読み込み",

      addPanelTitle: "新規ガジェット追加",
      fieldType: "種別",
      typeUrl: "Web URL",
      typeFile: "ローカル HTML",
      fieldTitleOpt: "タイトル（任意）",
      titlePlaceholder: "例: 付箋メモ＆ToDo",
      fieldSource: "URL または HTML ファイルパス",
      sourcePlaceholderUrl: "https://example.com/...",
      sourcePlaceholderFile: "C:\\path\\to\\widget.html",
      browseBtn: "参照...",
      addAndShowBtn: "追加して表示",
      dialogUnavailable: "ファイル選択ダイアログが利用できません。",
      htmlFilesFilter: "HTML ファイル",
      filePickError: "ファイル選択エラー: {err}",
      emptySourceError: "URL またはファイルパスを入力してください。",
      widgetAdded: "ガジェットを追加しました。",
      widgetAddError: "追加エラー: {err}",
      widgetsReloaded: "表示中のガジェットを再読み込みしました。",

      registeredPanelTitle: "登録済みガジェット一覧",
      noWidgets: "ガジェットはまだ登録されていません。",
      visibleToggle: "表示",
      toggleError: "切替エラー: {err}",
      boundX: "X",
      boundY: "Y",
      boundW: "幅",
      boundH: "高",
      applyBtn: "適用",
      boundsUpdated: "「{title}」の位置・サイズを更新しました。",
      boundsUpdateError: "更新エラー: {err}",
      duplicateBtn: "＋ 増やす",
      duplicateTitle: "同じウィジェットの空の新規インスタンスを追加",
      duplicateCreated: "「{title}」を新規追加しました。",
      duplicateError: "複製エラー: {err}",
      reloadBtn: "再読込",
      centerBtn: "中央へ戻す",
      centeredStatus: "「{title}」の位置を中央へ戻しました。",
      deleteBtn: "削除",
      deletedStatus: "「{title}」を削除しました。",

      settingsUpdateError: "設定更新エラー: {err}",
      flagLock: "ロック",
      flagClickThrough: "クリック透過",
      flagNoActivate: "フォーカス非奪取",
      labelOpacity: "透明度",
      labelZoom: "ズーム",
      labelAutoReload: "自動更新",
      reloadOff: "無効",
      reloadMinutes: "{mins}分",
      labelPlacement: "配置",
      modeBottom: "モードA: 最背面固定",
      modeWorkerW: "モードB: 壁紙埋め込み (WorkerW)",
      customCssActive: "カスタム CSS（設定あり）",
      customCssLabel: "カスタム CSS",
      customCssPlaceholder: "例: body { background: transparent !important; }",
      applyCssBtn: "CSS 適用",
      customCssApplied: "「{title}」にカスタム CSS を適用しました。",

      ddConfigMissingSummary: "⚙ ウィジェット設定（DD.config・未設定項目あり）",
      ddConfigSummary: "⚙ ウィジェット設定（DD.config）",
      requiredBracket: " [必須]",
      saveConfigBtn: "設定を保存して反映",
      savingText: "保存中...",
      configSavedInline: "保存し、ガジェットへ反映しました。",
      configSavedStatus: "「{title}」のウィジェット設定を保存して反映しました。",
      configSaveInlineError: "保存エラー: {err}",
      configSaveStatusError: "ウィジェット設定保存エラー: {err}",

      connPanelTitle: "外部サービス接続（SaaS 連携）",
      addConnBtn: "＋ 接続を追加",
      notionGuideSummary: "📖 Notion 連携の設定手順（4ステップ）",
      notionGuideStep1:
        "<code>https://app.notion.com/developers/connections</code> で <strong>+ New connection</strong> → <strong>名前を入力</strong>（空欄だと Create ボタン無効）→ Authentication method で <strong>API token</strong>（旧 Internal Integration 相当）を選んで <strong>Create connection</strong>",
      notionGuideStep2:
        "Capabilities で <strong>Read content</strong> に加え、チェック書き戻しを行う場合は <strong>Update content</strong> を確認し、発行されたトークン（<code>ntn_...</code>）を下の「＋ 接続を追加」から保存（送信先 <code>api.notion.com</code>、Windows 資格情報マネージャー保管）",
      notionGuideStep3:
        "Notion 側の対象データベース右上の <code>⋯</code> → <code>コネクト (Connections)</code> からコネクトを追加、または開発者コンソールの <code>Content access</code> タブから親ページを選択して共有（※未共有だと <code>object_not_found</code> エラー）",
      notionGuideStep4:
        "対象 DB に <strong>タイトル (<code>title</code>)</strong>・<strong>日付 (<code>date</code>)</strong>・<strong>完了チェック (<code>checkbox</code>)</strong> の 3 プロパティを用意し、ガジェットの「⚙ ウィジェット設定」へ DB ID（または DB URL）を入力して保存",
      openNotionConsoleLink: "Notion 開発者コンソール (Connections) を開く ↗",
      openBrowserError: "外部ブラウザ起動エラー: {err}",

      connFormTitleAdd: "新しい外部サービス接続を追加",
      connFormTitleEdit: "接続を編集: {id}",
      closeBtn: "閉じる",
      fieldConnector: "コネクタ種別",
      fieldConnId: "接続 ID（例: notion, notion_work）",
      connIdPlaceholder: "例: notion",
      fieldConnName: "表示名",
      connNamePlaceholder: "例: Notion（個人用）",
      connDestTitle: "送信先・保管先確認",
      connDestUrlLabel: "送信先 URL: ",
      connDestHostsLabel: "許可ホスト: ",
      connDestKeyLabel: "資格情報マネージャー保管キー: ",
      saveConnBtn: "接続を保存",
      saveChangesBtn: "変更を保存",
      badgeOfficialLong: "✓ 公式 (Official)",
      badgeCustom: "カスタム",
      badgeOfficialShort: "公式",
      connectorMissingDef: "{id} (定義未検出)",
      noCredFieldsNote: "このコネクタには追加の認証フィールド定義がありません。",
      credStatusSaved: "登録済み",
      credStatusUnset: "未登録",
      credStatusRequired: "必須",
      credStatusOptional: "任意",
      btnShowSecret: "表示",
      btnHideSecret: "隠す",
      credReplaceHint: "差し替える場合のみ入力（空欄なら既存の秘密を維持）",
      errEnterConnId: "接続 ID を入力してください。",
      errSelectConnector: "コネクタ種別を選択してください。",
      connSavedStatus: "接続「{id}」を保存しました（資格情報マネージャーへ安全に保管）。",
      connSaveError: "接続保存エラー: {err}",
      connectorsFetchError: "コネクタ一覧取得エラー: {err}",
      connectionsFetchError: "接続一覧取得エラー: {err}",
      noConnections: "外部サービス接続はまだ登録されていません。「＋ 接続を追加」から設定してください。",
      connCredSavedBadge: "資格情報: 登録済み",
      connCredUnsetBadge: "資格情報: 未登録",
      connAllowedHostsInline: " (許可ホスト: {hosts})",
      connDestLine: "送信先: {url}{hosts}",
      noneLabel: "なし",
      connKeysLine: "保管キー: {keys}",
      hintPrefix: "ヒント: {hint}",
      testConnBtn: "接続テスト",
      testConnPassStatus: "接続「{id}」テスト成功: {summary}",
      testConnFailStatus: "接続「{id}」テスト失敗: {summary}",
      testExecErrorSummary: "テスト実行エラー",
      testConnErrorStatus: "接続テストエラー: {err}",
      editConnBtn: "編集（トークン差替）",
      connDeletedStatus: "接続「{id}」と資格情報マネージャーの秘密を削除しました。",
      connDeleteError: "削除エラー: {err}",

      autostartLabelHtml: "Windows 起動時に自動起動（<code>--silent</code> でトレイ常駐）",
      autostartEnabled: "Windows 起動時の自動起動を有効にしました。",
      autostartDisabled: "Windows 起動時の自動起動を無効にしました。",
      autostartError: "自動起動設定エラー: {err}",
      webviewProfileLabel: "WebView2 プロファイル: ",
      loadingText: "読み込み中...",
      fetchFailed: "取得失敗: {err}",
    },
    en: {
      docTitle: "DesktopDashboards - Manager",
      langLabel: "Language",
      langAuto: "Auto (System)",
      langJa: "日本語",
      langEn: "English",
      langUpdated: "Display language switched to English.",
      reloadAll: "Reload All",

      addPanelTitle: "Add New Widget",
      fieldType: "Type",
      typeUrl: "Web URL",
      typeFile: "Local HTML",
      fieldTitleOpt: "Title (optional)",
      titlePlaceholder: "e.g. Sticky Notes & ToDo",
      fieldSource: "URL or HTML File Path",
      sourcePlaceholderUrl: "https://example.com/...",
      sourcePlaceholderFile: "C:\\path\\to\\widget.html",
      browseBtn: "Browse...",
      addAndShowBtn: "Add & Show",
      dialogUnavailable: "File picker dialog is not available.",
      htmlFilesFilter: "HTML Files",
      filePickError: "File selection failed: {err}",
      emptySourceError: "Please enter a URL or HTML file path.",
      widgetAdded: "Widget added.",
      widgetAddError: "Failed to add widget: {err}",
      widgetsReloaded: "Reloaded all active widgets.",

      registeredPanelTitle: "Registered Widgets",
      noWidgets: "No widgets registered yet.",
      visibleToggle: "Visible",
      toggleError: "Toggle failed: {err}",
      boundX: "X",
      boundY: "Y",
      boundW: "W",
      boundH: "H",
      applyBtn: "Apply",
      boundsUpdated: "Updated position & size for \"{title}\".",
      boundsUpdateError: "Update failed: {err}",
      duplicateBtn: "+ Duplicate",
      duplicateTitle: "Add a new blank instance of this widget",
      duplicateCreated: "Created new widget \"{title}\".",
      duplicateError: "Duplicate failed: {err}",
      reloadBtn: "Reload",
      centerBtn: "Center",
      centeredStatus: "Centered \"{title}\".",
      deleteBtn: "Delete",
      deletedStatus: "Deleted \"{title}\".",

      settingsUpdateError: "Failed to update settings: {err}",
      flagLock: "Lock",
      flagClickThrough: "Click-through",
      flagNoActivate: "No Focus",
      labelOpacity: "Opacity",
      labelZoom: "Zoom",
      labelAutoReload: "Auto Reload",
      reloadOff: "Off",
      reloadMinutes: "{mins}m",
      labelPlacement: "Placement",
      modeBottom: "Mode A: Keep at Bottom",
      modeWorkerW: "Mode B: Wallpaper Layer (WorkerW)",
      customCssActive: "Custom CSS (Active)",
      customCssLabel: "Custom CSS",
      customCssPlaceholder: "e.g. body { background: transparent !important; }",
      applyCssBtn: "Apply CSS",
      customCssApplied: "Applied custom CSS to \"{title}\".",

      ddConfigMissingSummary: "⚙ Widget Settings (DD.config — setup required)",
      ddConfigSummary: "⚙ Widget Settings (DD.config)",
      requiredBracket: " [Required]",
      saveConfigBtn: "Save & Apply",
      savingText: "Saving...",
      configSavedInline: "Saved and applied to widget.",
      configSavedStatus: "Saved and applied widget settings for \"{title}\".",
      configSaveInlineError: "Save failed: {err}",
      configSaveStatusError: "Failed to save widget settings: {err}",

      connPanelTitle: "External Service Connections (SaaS)",
      addConnBtn: "+ Add Connection",
      notionGuideSummary: "📖 Notion Integration Setup Guide (4 Steps)",
      notionGuideStep1:
        "Go to <code>https://app.notion.com/developers/connections</code> → click <strong>+ New connection</strong> → enter a <strong>name</strong> (Create stays disabled if empty) → choose <strong>API token</strong> (Internal Integration) and click <strong>Create connection</strong>",
      notionGuideStep2:
        "Under Capabilities, verify <strong>Read content</strong> (plus <strong>Update content</strong> if toggling task checkboxes), then copy the token (<code>ntn_...</code>) and save it below via \"+ Add Connection\" (sent only to <code>api.notion.com</code>; stored in Windows Credential Manager)",
      notionGuideStep3:
        "In Notion, open your target database's top-right <code>⋯</code> → <code>Connections</code> and add your integration, or share its parent page under the developer console's <code>Content access</code> tab (missing access causes <code>object_not_found</code>)",
      notionGuideStep4:
        "Ensure the database has <strong>Title (<code>title</code>)</strong>, <strong>Date (<code>date</code>)</strong>, and <strong>Checkbox (<code>checkbox</code>)</strong> properties, then enter the DB ID (or URL) in \"⚙ Widget Settings\" on the widget card below",
      openNotionConsoleLink: "Open Notion Developer Console (Connections) ↗",
      openBrowserError: "Failed to open external browser: {err}",

      connFormTitleAdd: "Add New Service Connection",
      connFormTitleEdit: "Edit Connection: {id}",
      closeBtn: "Close",
      fieldConnector: "Connector Type",
      fieldConnId: "Connection ID (e.g. notion, notion_work)",
      connIdPlaceholder: "e.g. notion",
      fieldConnName: "Display Name",
      connNamePlaceholder: "e.g. Notion (Personal)",
      connDestTitle: "Destination & Credential Storage",
      connDestUrlLabel: "Destination URL: ",
      connDestHostsLabel: "Allowed Hosts: ",
      connDestKeyLabel: "Credential Manager Key: ",
      saveConnBtn: "Save Connection",
      saveChangesBtn: "Save Changes",
      badgeOfficialLong: "✓ Official",
      badgeCustom: "Custom",
      badgeOfficialShort: "Official",
      connectorMissingDef: "{id} (Definition missing)",
      noCredFieldsNote: "No additional credential fields are defined for this connector.",
      credStatusSaved: "Saved",
      credStatusUnset: "Not set",
      credStatusRequired: "Required",
      credStatusOptional: "Optional",
      btnShowSecret: "Show",
      btnHideSecret: "Hide",
      credReplaceHint: "Enter only if replacing (leave blank to keep existing secret)",
      errEnterConnId: "Please enter a connection ID.",
      errSelectConnector: "Please select a connector type.",
      connSavedStatus: "Saved connection \"{id}\" securely in Windows Credential Manager.",
      connSaveError: "Failed to save connection: {err}",
      connectorsFetchError: "Failed to load connectors: {err}",
      connectionsFetchError: "Failed to load connections: {err}",
      noConnections: "No external service connections registered yet. Click \"+ Add Connection\" to add one.",
      connCredSavedBadge: "Credentials: Saved",
      connCredUnsetBadge: "Credentials: Not set",
      connAllowedHostsInline: " (Allowed hosts: {hosts})",
      connDestLine: "Destination: {url}{hosts}",
      noneLabel: "None",
      connKeysLine: "Stored keys: {keys}",
      hintPrefix: "Hint: {hint}",
      testConnBtn: "Test",
      testConnPassStatus: "Connection \"{id}\" test passed: {summary}",
      testConnFailStatus: "Connection \"{id}\" test failed: {summary}",
      testExecErrorSummary: "Test execution error",
      testConnErrorStatus: "Connection test error: {err}",
      editConnBtn: "Edit (Replace Token)",
      connDeletedStatus: "Deleted connection \"{id}\" and its stored credentials.",
      connDeleteError: "Delete failed: {err}",

      autostartLabelHtml: "Launch on Windows startup (tray resident with <code>--silent</code>)",
      autostartEnabled: "Enabled launch on Windows startup.",
      autostartDisabled: "Disabled launch on Windows startup.",
      autostartError: "Startup setting error: {err}",
      webviewProfileLabel: "WebView2 Profile: ",
      loadingText: "Loading...",
      fetchFailed: "Failed to load: {err}",
    },
  };

  const BUNDLED_MANIFEST_SCHEMAS = {
    "notion-today": {
      en: {
        connection: {
          label: "Connection ID",
          placeholder: "notion",
          description: "Connection ID registered under \"External Service Connections\" in Manager",
        },
        database_id: {
          label: "Notion Database ID (or URL)",
          placeholder: "32-char ID or https://www.notion.so/...",
          description: "Paste either the database ID or its full Notion URL",
        },
        date_property: {
          label: "Date Property Name (optional)",
          placeholder: "Auto-detect (date property)",
          description: "Leave blank to auto-detect the database's date property",
        },
        done_property: {
          label: "Done Property Name (optional)",
          placeholder: "Auto-detect (checkbox property)",
          description: "Leave blank to auto-detect the database's checkbox property",
        },
        show_done: {
          label: "Show completed tasks at the bottom",
        },
        show_overdue: {
          label: "Also show overdue incomplete tasks",
        },
      },
    },
  };

  let currentLang = /^ja\b/i.test(navigator.language || "") ? "ja" : "en";

  function normalizeLang(lang) {
    return lang === "ja" ? "ja" : "en";
  }

  function setLang(lang) {
    currentLang = normalizeLang(lang);
    document.documentElement.lang = currentLang;
    document.title = t("docTitle");
  }

  function getLang() {
    return currentLang;
  }

  function t(key, vars = null) {
    const dict = TRANSLATIONS[currentLang] || TRANSLATIONS.en;
    const fallback = TRANSLATIONS.en;
    let template = Object.prototype.hasOwnProperty.call(dict, key)
      ? dict[key]
      : Object.prototype.hasOwnProperty.call(fallback, key)
        ? fallback[key]
        : key;
    if (vars && typeof vars === "object") {
      template = String(template).replace(/\{(\w+)\}/g, (_, k) =>
        Object.prototype.hasOwnProperty.call(vars, k) && vars[k] !== undefined && vars[k] !== null
          ? String(vars[k])
          : ""
      );
    }
    return template;
  }

  function translateManifestField(manifestId, field) {
    if (!field || typeof field !== "object") {
      return { label: "", placeholder: "", description: "" };
    }
    const override = BUNDLED_MANIFEST_SCHEMAS[manifestId]?.[currentLang]?.[field.key];
    return {
      label: override?.label ?? field.label ?? field.key ?? "",
      placeholder: override?.placeholder ?? field.placeholder ?? "",
      description: override?.description ?? field.description ?? "",
    };
  }

  function applyI18n(root = document) {
    document.documentElement.lang = currentLang;
    document.title = t("docTitle");
    root.querySelectorAll("[data-i18n]").forEach((el) => {
      el.textContent = t(el.dataset.i18n);
    });
    root.querySelectorAll("[data-i18n-html]").forEach((el) => {
      el.innerHTML = t(el.dataset.i18nHtml);
    });
    root.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
      el.placeholder = t(el.dataset.i18nPlaceholder);
    });
    root.querySelectorAll("[data-i18n-title]").forEach((el) => {
      el.title = t(el.dataset.i18nTitle);
    });
  }

  window.DD_I18N = {
    setLang,
    getLang,
    t,
    translateManifestField,
    applyI18n,
  };
})();
