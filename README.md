# DesktopDashboards

[![Release](https://img.shields.io/badge/version-v0.1.0-blue.svg)](https://github.com/mowthow-works/DesktopDashboards/releases)
[![Platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6.svg)](https://www.microsoft.com/windows)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

任意の Web アプリ、AI アーティファクト（Muse Spaces 等）、ローカル HTML ファイルを Windows のデスクトップ（最背面／壁紙レイヤー）に複数並べて常駐表示できる、Tauri v2 + WebView2 製の軽量デスクトップガジェットアプリです。

## 主な機能（Phase 1〜3 実装済み）

- **汎用 Web デスクトップガジェット**: Web URL およびローカル HTML ファイル（`widgetfile` カスタムプロトコル経由）をデスクトップウィジェットとして表示
- **セッション維持・分離**: 専用の WebView2 `user_data_dir`（`%LOCALAPPDATA%\DesktopDashboards\webview`）を使用し、他アプリとのロック競合を防ぎつつガジェット間でログイン状態を共有
- **外部アプリ連携・多重起動防止**: `tauri-plugin-single-instance` と CLI 引数によるシームレスな呼び出し
  - `DesktopDashboards.exe`（引数なし）: 登録済みガジェットを復元・管理画面を表示（既に起動中なら管理画面を前面表示）
  - `DesktopDashboards.exe --manager`: 管理画面を前面表示
  - `DesktopDashboards.exe --add-url <URL> [--title <NAME>]`: 指定 URL を追加・有効化して表示
  - `DesktopDashboards.exe --silent`: 管理画面を開かずトレイ常駐＋有効ガジェットのみ静かに起動
- **デスクトップ固定・ナビゲーション制御**:
  - **モード A（最背面固定）**: 枠なし・タスクバー非表示・最背面固定（`HWND_BOTTOM` + `WS_EX_TOOLWINDOW` + `WM_WINDOWPOSCHANGING`）
  - **モード B（壁紙レイヤー埋め込み・Phase 3）**: `Progman` / `WorkerW` へ埋め込むことで `Win + D` 耐性を実現（ガジェット個別に切替可・Explorer 再起動／壁紙変更時の自動再アタッチ対応）
  - **ナビゲーション制御**: 同一オリジン内遷移許可・外部リンクの既定ブラウザ起動
- **ガジェット操作・カスタマイズ（Phase 2〜3）**:
  - **ロック機能**: ロック解除時は上端ホバーで操作バー（ドラッグ移動・ホーム・リロード・ロック・閉じる）を表示しリサイズ可能、ロック時はバー・リサイズを無効化（トレイ「すべてロック／解除」対応）
  - **透明度＋クリック透過**: `WS_EX_LAYERED`（20%〜100%）および `WS_EX_TRANSPARENT` によるマウス透過
  - **フォーカス非奪取**: `WS_EX_NOACTIVATE` によりクリックしても他アプリのフォーカスを奪わない
  - **ズーム倍率**: ガジェットごとに 50%〜200% の倍率設定
  - **定期自動リロード＋リロードガード**: 1〜60分間隔の自動リロードと、入力欄フォーカス中・操作直後のリロード延期保護
  - **カスタム CSS 注入（Phase 3）**: ガジェットごとに任意の CSS を編集・即時適用・永続化
- **設定の原子的永続化＆自動起動の自動復元**: `%APPDATA%\DesktopDashboards\config.json` への原子的書き込み（`.tmp` + flush + リネーム）および 1 世代バックアップ（`.bak`）フォールバック。Windows 自動起動設定も `config.json` に保存され、バージョンアップ（上書きインストール）等で OS 側スタートアップ登録が消去されても次回起動時に自動復元
- **管理画面 & システムトレイ常駐**: ガジェットの追加・表示切替・位置サイズ調整・各種詳細設定（`DD.config` フォーム自動生成含む）・削除、外部サービス接続（SaaS 連携）の一覧・追加（送信先明示＋`credential_schema` 動的フォーム）・トークン差し替え・削除・接続テスト、Windows 起動時の自動起動（`--silent`）トグル
- **NSIS インストーラ対応**: 日本語・英語に対応した Per-user インストーラおよびポータブル ZIP 配布に対応

## 📥 インストール

[Releases](https://github.com/mowthow-works/DesktopDashboards/releases) からインストーラ（`DesktopDashboards_x64-setup.exe`）またはポータブル版 ZIP（`DesktopDashboards-windows-x64.zip`）をダウンロードして実行してください。インストーラは管理者権限不要（Per-user インストール）で、デスクトップショートカット・スタートメニュー登録・サンプル HTML 群（`samples/`）の同梱とアンインストーラが自動構成されます。

- **インストーラ版（推奨）**: `DesktopDashboards_x64-setup.exe` を実行してインストール後、デスクトップショートカットまたはスタートメニューから起動します。
- **ポータブル版**: `DesktopDashboards-windows-x64.zip` を任意のフォルダに展開し、中の `DesktopDashboards.exe` をダブルクリックするだけで起動できます。

### Windows SmartScreen 警告が出た場合

ダウンロードしたインストーラやポータブル版の実行時に **「Windows によって PC が保護されました」** という青い画面が表示される場合があります。これは、個人開発・オープンソースの新しいバイナリに対して SmartScreen が「ダウンロード実績がまだ少ない未知のアプリ」として機械的に表示する安全確認の画面です。以下の手順でそのままインストール・起動できます。

<div align="center">
  <img src="docs/smartscreen_warning.png" alt="Windows SmartScreen 警告画面" width="340" />
</div>

1. 警告画面の左上にある **「詳細情報」**（下線リンク）をクリックします。
2. 画面右下に現れる **「実行」** ボタンをクリックします。
3. 通常通りセットアップ画面（またはアプリ）が起動します。

> [!TIP]
> ダウンロードした `.exe` ファイルを右クリック ➜ **「プロパティ」** を開き、全般タブの一番下にある **「セキュリティ: 許可する」**（または「ブロックの解除」）にチェックを入れて「OK」を押すことでも警告を回避できます。

## ローカル HTML ガジェットの利用方法と同梱サンプル

Web URL だけでなく、ローカルに作成した HTML ファイルをデスクトップガジェットとして表示できます。初回起動時（設定ファイル未作成時）は、同梱サンプルの **「付箋メモ＆ToDo」（`samples/sticky-todo.html`）** がデフォルトガジェットとして 1 件登録・表示されます。

1. 管理画面の **「新規ガジェット追加」** で種別を **「ローカル HTML」** に切り替えます。
2. **「参照...」** ボタンを押すと、同梱のサンプルフォルダ（`samples/`）が開きます。任意の `.html` / `.htm` ファイルを選択して **「追加して表示」** をクリックします。

### 同梱サンプル HTML（`samples/`）

インストーラにも同梱されており、そのままガジェットとして試したり、複製して自作ガジェットのベースにしたりできます。

- `samples/sticky-todo.html`（＋`samples/sticky-todo.manifest.json`）: **付箋メモ＆ToDoリスト** — `localStorage` による即時自動保存・インスタンス分離（複数追加可）・リロードガード・フォーカス非奪取の体験用サンプル
- `samples/pomodoro-timer.html`: **ポモドーロ集中タイマー** — 円形プログレスリング・チャイム通知（消音可）・当日完了数記録付きの作業用タイマー
- `samples/starter-template.html`: **自作ガジェット用スターターテンプレート** — 上端 28px ホバーバー余白、同一フォルダ内の相対パス読み込み、外部リンクの既定ブラウザ起動、`localStorage` テスト、および manifest 無し制限モード確認付き
- `samples/notion-today.html`（＋`samples/notion-today.manifest.json`）: **Notion 今日のタスク（公式サンプル1号）** — `DD.config.get()` による設定管理（複数 DB の並行配置にも対応）、Notion DB からの当日／期限切れ未完了タスク抽出、チェックボックス双方向書き戻し（`PATCH /v1/pages/*`）、タスククリックでの `DD.openUrl()` ブラウザ起動に対応
- `samples/notion-spike.html`（＋`samples/notion-spike.manifest.json`）: **Notion API (`2025-09-03`) & `window.DD` ブリッジ検証サンプル** — `DD.storage`（ウィジェット分離ストレージ）と `DD.connect.request`（Windows 資格情報マネージャー連携・権限積集合判定）の動作確認用サンプル

### ローカル HTML 作成時のポイント

- **相対パス資産**: 選択した HTML と同じフォルダおよびサブフォルダ内の CSS / JS / 画像 / JSON 等は相対パスで読み込めます（`widgetfile` カスタムプロトコル経由・親ディレクトリ外への参照は遮断）。
- **上端余白**: ロック解除時は上端 28px にホバー操作バーが表示されるため、`body` に `padding-top: 32px` 程度を確保すると操作しやすくなります。
- **外部リンク**: `http://` / `https://` へのリンクはガジェット内ではなく OS の既定ブラウザで開きます。スクリプトから既定ブラウザを開く場合は `DD.openUrl(url)`（`http://` / `https://` 限定）を利用できます。
- **`localStorage` のインスタンス分離**: ローカル HTML ガジェット（`SourceType::File`）のトップフレームではホスト側初期化スクリプトのシムが `widget_id` ごとに `localStorage` の名前空間を自動分離します。そのため `localStorage` を使う無改造の HTML でも複数枚並べて独立したデータを保持できます（分離導入前から登録されている既存ガジェットは、初回アクセス時に従来のキー値を非破壊で引き継ぎます）。
- **複製可能ウィジェット（`"duplicable": true`）**: `<html_stem>.manifest.json` または `manifest.json` に `"duplicable": true` を宣言すると、ガジェット上端ホバーバーの `＋` ボタンおよび管理画面カードの `＋ 増やす` ボタンから、同じ HTML の空の新規インスタンス（連番タイトル・カスケード配置・`DD.storage` / `widget_config` / `localStorage` 初期状態）を追加できます（未宣言時は非表示、URL ガジェットは対象外）。
- **`window.DD` ブリッジ（Phase 4b〜4c）**: ローカル HTML ガジェットのトップフレーム（`http://widgetfile.localhost`）にのみ注入され、ウィジェット単位の分離ストレージ（`DD.storage.get` / `set`、`%APPDATA%\DesktopDashboards\storage\{widget_id}.json` 保存）、ガジェット設定取得（`DD.config.get()`）、宣言的コネクタ経由の API ブリッジ（`DD.connect.request`）、および外部ブラウザ起動（`DD.openUrl`）を利用できます。URL ガジェットや外部 `<iframe>` からの呼び出しは Tauri ACL および Rust 側 `Origin` 検証で遮断されます。
  - **ガジェット設定フォーム自動生成（`DD.config`）**: ウィジェット manifest の `config_schema`（`string` / `boolean`）を定義すると、管理画面の各ガジェットカード内に「⚙ ウィジェット設定（`DD.config`）」フォームが自動生成されます。保存値は `%APPDATA%\DesktopDashboards\widget_config\{widget_id}.json` へ保存され、保存時にガジェットが自動リロードされて `await DD.config.get()` から取得できます。
  - **権限積集合と制限モード**: 同梱コネクタ定義（`connectors/notion.json` 等および `%APPDATA%\DesktopDashboards\connectors\*.json`）と、HTML と同じフォルダのウィジェット manifest（`<html_stem>.manifest.json` → `manifest.json`）双方の `endpoints` に一致するリクエストのみ許可されます。manifest が無いローカル HTML は制限モードとなり、`DD.storage` のみ利用できます（`DD.connect` は拒否）。
  - **管理画面での接続管理 & 資格情報保管**: 管理画面の「外部サービス接続（SaaS 連携）」パネルから公式印・送信先（`base_url` / `allowed_hosts`）を確認しながら接続の追加・編集（トークン差し替え）・削除・接続テストを行えます。API トークン等の秘密情報は Windows 資格情報マネージャー（`DesktopDashboards:connection:<connection_id>:<credential_key>`）へ保管・削除され、`%APPDATA%\DesktopDashboards\connections.json`（v2 形式）には秘密を含まないメタ情報のみを保持します（旧 v1 平文トークンがある場合は起動時に自動移行・消去）。

### 「Notion 今日のタスク」設定ガイド

`samples/notion-today.html`（＋`samples/notion-today.manifest.json`）を利用して、Notion データベースの今日のタスクをデスクトップに表示・チェック更新する手順です（2026-11-18 実機確認済みの新 Notion 開発者コンソール画面に対応）。

1. **Notion インテグレーションと API トークンの作成**:
   - [Notion 開発者コンソール (Connections)](https://app.notion.com/developers/connections) を開き、**`+ New connection`** をクリックします。
   - 接続名（**名前を入力必須** — 空欄のままだと Create ボタンが無効になります）を入力し、Authentication method で **`API token`**（旧 Internal Integration 相当）を選択して **`Create connection`** をクリックします。
   - Capabilities タブで **`Read content`** に加え、チェックボックスの双方向書き戻しを行う場合は **`Update content`** が有効になっていることを確認し、発行されたシークレットトークン（`ntn_...`）をコピーします。
2. **対象データベースの準備と共有（2経路のいずれか）**:
   - 対象 Notion データベースには **タイトル（`title` 型）**・**日付（`date` 型）**・**完了チェック（`checkbox` 型）** の 3 つのプロパティを用意してください。
   - インテグレーションへデータベースを共有します（未共有のままだと `object_not_found` エラーになります）：
     - **経路 A**: Notion で対象データベースを開き、右上の **`⋯`** → **`コネクト (Connections)`** から作成したインテグレーションを追加する。
     - **経路 B**: Notion 開発者コンソールの **`Content access`** タブから、対象データベースまたは親ページを選択して共有する。
3. **DesktopDashboards 管理画面で接続を登録**:
   - 管理画面の「外部サービス接続（SaaS 連携）」→ **「＋ 接続を追加」** を開き、コネクタ種別 **Notion**（送信先 `https://api.notion.com`、許可ホスト `api.notion.com` であることを確認）を選択します。
   - 接続 ID（例: `notion`）と表示名を確認し、コピーした `ntn_...` トークンを貼り付けて保存します（トークンは Windows 資格情報マネージャーに暗号化保管されます）。保存後、一覧カードの「接続テスト」で疎通確認できます。
4. **ガジェットの追加と `DD.config` 設定**:
   - 「新規ガジェット追加」で種別 **「ローカル HTML」** を選び、`samples/notion-today.html` を追加して表示します。
   - 登録済みガジェットカードの **「⚙ ウィジェット設定（`DD.config`）」** を開き、**接続 ID**（例: `notion`）と **Notion データベース ID（または DB URL 全体）** を入力して **「設定を保存して反映」** をクリックします（`date_property` / `done_property` は省略時は自動検出されます）。

## 技術スタック

- **対象 OS**: Windows 10 / 11 (x64)
- **フレームワーク**: Tauri v2 (Rust) + WebView2 + HTML/CSS/JS

## 💻 開発・ビルド方法

### 開発環境の前提条件
- **Node.js**: v20以上
- **Rust**: 1.80以上 (MSVC ツールチェーン)
- **Visual Studio C++ Build Tools**

### 開発サーバー起動・単体テスト

```powershell
# 依存パッケージのインストール
npm install

# 開発モードで起動
npm run dev

# Rust バックエンド単体テスト
npm run test:rust
```

### プロダクションビルド

ポータブル単体実行ファイル（手動配置・ZIP 配布向け）：
```powershell
cargo build --release --manifest-path "src-tauri/Cargo.toml"
# 生成先: src-tauri/target/release/desktop-dashboards.exe
```

NSIS セットアップインストーラ（一般配布向け）：
```powershell
npm run build:installer
# または
npx tauri build
# 生成先: src-tauri/target/release/bundle/nsis/DesktopDashboards_<version>_x64-setup.exe
```

## ドキュメント

- [仕様書 (docs/SPEC.md)](docs/SPEC.md)

## 📜 ライセンス

[MIT License](LICENSE) © 2026 mowthow-works
