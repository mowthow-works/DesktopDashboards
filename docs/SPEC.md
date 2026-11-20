# DesktopDashboards 仕様書（SPEC・v0.2.0 実装反映版）

- **リポジトリ名**: `DesktopDashboards`
- **アプリ愛称**: 未定（仮称：`DesktopDashboards`）
- **対象 OS**: Windows 10 / 11 (x64)
- **技術スタック**: Tauri v2 (Rust) + WebView2 + HTML/CSS/JS

---

## 1. コンセプト・目的

任意の Web アプリ、AI アーティファクト（Muse Spaces 等）、ローカル HTML ファイルを、**Windows のデスクトップ（壁紙／最背面）に複数並べて常駐表示できる軽量デスクトップガジェットアプリ**。

- **独立プロダクトとしての汎用性**: 特定の Web サービスに依存せず、URL やローカル HTML を登録するだけであらゆる Web 画面をデスクトップウィジェット化できる。
- **外部アプリからのキック連携**: 単体起動だけでなく、`Muse Desktop Mascot` などの外部アプリにバンドルし、CLI 引数付きで呼び出すことでシームレスに連携できる。

---

## 2. システムアーキテクチャ

### 2.1 プロセス・ウィンドウ構成（1プロセス・マルチウィンドウ）

トレイ常駐型の **単一プロセス** で動作し、その中に複数のウィンドウを生成・管理する。

1. **システムトレイ（常駐本体）**: ウィンドウをすべて閉じてもバックグラウンドで常駐可能。
2. **管理画面ウィンドウ（Manager UI / 1つ）**: ガジェットの追加・一覧管理・各種設定を行う通常ウィンドウ。
3. **ガジェットウィンドウ（Widget WebView / 0〜N個）**: 登録・有効化されたコンテンツを表示する枠なしデスクトップガジェットウィンドウ。
   - **メモリ管理方針**: WebView2 はウィンドウごとにレンダラプロセスが立ち上がるため、同時表示の快適動作目安は **5個程度まで** と想定する。`enabled: false`（非表示）のガジェットはウィンドウを隠す（hide）のではなく **破棄（close）** してメモリを解放する。

### 2.2 WebView2 セッション分離

- 他の Tauri / WebView2 アプリとのファイルロック競合を防ぐため、専用の `user_data_dir` を明示的に使用する。
  - 例：`%LOCALAPPDATA%\DesktopDashboards\webview`
- 同一プロセス内の複数ガジェットウィンドウ間では同じ `user_data_dir` を共有するため、**一度ログインしたサービス（例：`muse.ai`）の認証状態は全ガジェット間で共有**される。

### 2.3 多重起動防止と CLI 連携

- `tauri-plugin-single-instance` を導入し、アプリの多重起動を防止する。
- すでに起動済みの状態で再実行された場合、2つ目のプロセスは引数のみを既存プロセスへ渡して即終了する。

#### CLI 引数仕様（外部アプリ連携用）

| コマンド例 | 動作 |
| :--- | :--- |
| `DesktopDashboards.exe`（引数なし） | 初回起動時は登録済みガジェットを復元。既に起動中なら管理画面を前面表示。 |
| `DesktopDashboards.exe --manager` | 管理画面ウィンドウを開いて前面表示。 |
| `DesktopDashboards.exe --add-url <URL> [--title <NAME>]` | 指定 URL を新規ガジェットとして追加して表示する。同一 URL が既に存在する場合は有効化し、一時的にZオーダーを引き上げて視認させた後（または管理画面上でハイライト）所定の位置に配置する。 |
| `DesktopDashboards.exe --silent` | 管理画面を開かず、トレイ常駐＋有効なガジェットのみ静かに起動する（**OS自動起動時は必ずこのフラグ付きで登録**する）。 |

---

## 3. 壁紙張り付き（デスクトップ固定）仕様

Windows 環境で「他ウィンドウの邪魔にならず、壁紙のように張り付く」挙動を実現するため、2つの固定モードを提供する。  
`AppConfig.placement_mode` が新規作成時の既定配置モードとなり、現行実装では `WidgetConfig.placement_mode` によりガジェット個別にも「最背面固定（Bottom）」と「壁紙レイヤー埋め込み（WorkerW）」を切り替えられる。

### 3.1 モード A：最背面固定モード（デフォルト・安定重視）

- `decorations: false`（タイトルバー・枠なし）、`skip_taskbar: true`（タスクバー非表示）。
- `always_on_bottom: true` に加え、Win32 API（`WS_EX_TOOLWINDOW` 付与による `Alt+Tab` 除外、および `WM_WINDOWPOSCHANGING` / `SetWindowPos(HWND_BOTTOM)`）で常に他ウィンドウの最背面に維持する。
- **メリット**: Windows 10/11 のバージョン差に影響されず安定動作し、クリック・スクロール操作が素直に効く。
- **既知の制限**: 通常のデスクトップウィンドウとして扱われるため、**`Win + D`（デスクトップを表示）を押すと他のウィンドウと同様に最小化（一時非表示）される**。

### 3.2 モード B：壁紙レイヤー埋め込みモード（`WorkerW` モード・Phase 3）

- `Progman` へ `0x052C` を送信して `WorkerW` を生成し、`SetParent` によりガジェットをデスクトップアイコン層（`SHELLDLL_DefView`）の背面／壁紙層に埋め込む。
- **メリット**: `Win + D`（デスクトップを表示）を押してもガジェットが消えない。
- **モード切替時の座標変換**: モード A（仮想スクリーン座標）とモード B（`WorkerW` 親ウィンドウ相対のクライアント座標）では座標空間が異なるため、モード切替時に `ScreenToClient` / `ClientToScreen` 相当の座標変換を行い、ウィジェットが画面外へ飛ばないようにする。

---

## 4. ガジェット機能仕様

### 4.1 データモデルと永続化（`config.json`）

設定ファイルは `%APPDATA%\DesktopDashboards\config.json` に保存する。

#### 原子的書き込み・バックアップ・自動起動復元・初回初期化
- 位置・サイズ変更時のデバウンス保存や設定変更時は、直接 `config.json` を上書きせず、**一時ファイル（`config.json.tmp`）へ書き込み・flush してからリネーム置換**する原子的書き込みを行う。
- 正常な設定を 1世代バックアップ（`config.json.bak`）として保持し、万が一破損時は自動フォールバックする。
- Windows 自動起動設定（`autostart`）も `config.json` に保存し、バージョンアップ（上書きインストール）等で OS 側のスタートアップ登録が消去された場合でも次回起動時に自動復元する。
- `config.json` と `config.json.bak` のどちらも存在しない初回起動時に限り、同梱サンプルの `samples/sticky-todo.html`（「付箋メモ＆ToDo」）を 1 件デフォルト登録する。

```json
{
  "version": 1,
  "placement_mode": "bottom",
  "autostart": false,
  "widgets": [
    {
      "id": "550e8400-e29b-41d4-a716-446655440000",
      "title": "通勤電車 時刻表",
      "source_type": "url",
      "source": "https://muse.ai/library/artifacts/tong-qin-dian-che-shi-ke-biao",
      "enabled": true,
      "x": 1400,
      "y": 40,
      "width": 480,
      "height": 760,
      "dpi_scale": 1.0,
      "opacity": 0.95,
      "zoom": 1.0,
      "locked": false,
      "click_through": false,
      "no_activate": false,
      "auto_reload_minutes": 0,
      "hide_scrollbar": true,
      "custom_css": "",
      "placement_mode": "bottom",
      "local_storage_isolated": true
    }
  ]
}
```

#### ローカル HTML の扱い（`source_type: "file"`）
- `source_type` は `"url"` または `"file"` とする。
- `"file"` の場合、`source` にローカル HTML ファイルのパスを保持し、登録済みカスタムプロトコル `widgetfile`（`http://widgetfile.localhost/{widget_id}/...`）経由で安全なスコープのもと配信・表示する。

### 4.2 座標系とマルチモニタ DPI 対応

- 座標（`x`, `y`）およびサイズ（`width`, `height`）は **物理ピクセル（仮想デスクトップ座標）** で記録し、保存時のモニタの `dpi_scale`（例：`1.0`, `1.25`, `1.5`）も併せて保持する。
- 復元時は、保存時の論理サイズ（`物理サイズ / 保存時 dpi_scale`）を基準に**復元先モニタの DPI に合わせて物理サイズを再計算**し、見かけの大きさが極端に変わらないようにする。
- 起動・復元時に接続中モニタの領域外（例：外部モニタ取り外し後）にある場合は、プライマリモニタ内の安全領域へ自動補正する。

### 4.3 リンククリック時のナビゲーション方針

枠なしガジェットウィンドウにはブラウザの「戻る」ボタンがないため、以下のルールでナビゲーションを制御する：

1. **同一オリジン内の遷移**: ガジェット内でそのまま遷移を許可する。
2. **外部ドメインへの遷移 / 新規ウィンドウ（`target="_blank"` 等）**: ガジェット内では開かず、OS の既定ブラウザで開く（`tauri-plugin-opener` 等）。
3. **ホバーバーの操作**:
   - **リロード（↻）**: 現在表示中のページを再読み込みする。
   - **ホーム（⌂）**: 登録された初期 `source`（URL / ファイル）へ戻る。

### 4.4 各ガジェットの制御・表示機能

1. **ロック機能（`locked`）**
   - **ロック解除時**：ガジェット上端にホバーした時だけ薄い操作バー（ドラッグ移動・タイトル・ホーム・リロード・ロック・閉じる）を表示し、ウィンドウ端でのリサイズも可能。
   - **ロック時**：操作バーとリサイズ枠を完全に隠し、誤操作による移動・サイズ変更を防止する。
2. **透明度（`opacity`）とクリック透過（`click_through`）**
   - どちらも Win32 のレイヤードウィンドウ（`WS_EX_LAYERED`）を前提に実装する。
   - **透明度（20% 〜 100%）**: `SetLayeredWindowAttributes`（`LWA_ALPHA`）でウィンドウ全体の不透明度を制御する。
   - **クリック透過**: `WS_EX_TRANSPARENT` を付与し、マウスイベントを背後のデスクトップへ透過させる（解除はトレイメニューまたは管理画面から行う）。
3. **フォーカス非奪取（`no_activate`）**
   - 「見るだけ／軽く触るだけ」のガジェット向けに、Win32 `WS_EX_NOACTIVATE` を付与できるオプションを用意する。クリックしても他アプリのフォーカス（IMEや作業中ウィンドウ）を奪わない。
4. **表示カスタマイズ**
   - **ズーム倍率（`zoom`）**: 50% 〜 200%
   - **スクロールバー非表示（`hide_scrollbar`）**: CSS 注入によりすっきりした見た目に調整
5. **定期自動リロードと入力保護（`auto_reload_minutes`）**
   - 0（無効）、1分、5分、15分、30分、60分などの間隔で自動リロードする。
   - **リロードガード**: WebView 内の入力要素（`input` / `textarea` / `contenteditable`）にフォーカスがある場合、または直近数秒以内にユーザー操作（クリック・キー入力）があった場合はリロードを延期し、操作終了後に安全に実行する。トレイメニューの「すべて再読み込み」も同様にリロードガードの対象とする。

---

## 5. UI / 操作導線

### 5.1 システムトレイメニュー

- **管理画面を開く**（ダブルクリックでも同様）
- **ガジェット一覧**（チェックボックスで表示/非表示を即切替）
- **すべてロック / すべてロック解除**
- **すべて再読み込み**
- **Windows起動時に自動起動**（トグル：有効時は `--silent` 付きで登録）
- **終了**

### 5.2 管理画面（Dashboard Manager）

- **新規ガジェット追加**: URL またはローカル HTML ファイルを選択し、タイトルを入力して即作成（「参照...」ダイアログは同梱 `samples/` を初期表示）。
- **ガジェットカード一覧**:
  - 表示 ON / OFF スイッチ
  - ロック / クリック透過 / フォーカス非奪取 トグル
  - 配置モード（最背面固定 / 壁紙レイヤー埋め込み）切替
  - 詳細設定（透明度スライダー、ズーム倍率、自動リロード間隔、スクロールバー非表示、カスタム CSS）
  - **⚙ ウィジェット設定（`DD.config`）**: manifest に `config_schema` があるローカル HTML ウィジェットで動的フォームを自動生成
  - **＋ 増やす**: manifest に `"duplicable": true` が宣言されたローカル HTML ウィジェットで新規インスタンスを追加
  - 「位置を中央にリセット」ボタン（画面外迷子対策）
  - 削除ボタン
- **外部サービス接続（SaaS 連携）**: 接続先一覧・追加（送信先明示＋`credential_schema` 動的フォーム）・トークン差し替え・削除・接続テスト・設定ガイド表示
- **システム設定**: Windows 起動時の自動起動（`--silent`）トグル

---

## 6. 外部アプリ連携（Muse Desktop Mascot バンドル想定）

`Muse Desktop Mascot` 側からは以下のイメージで連携する：

1. `Muse Desktop Mascot` のビルド／配布パッケージ内に `DesktopDashboards.exe` を同梱する。
2. `Muse Desktop Mascot` のトレイメニューまたはチャット画面アクションから、`DesktopDashboards.exe --add-url <スペースURL>` または引数なしで起動する。
3. `DesktopDashboards` は独立した常駐アプリとして動作し、マスコットを終了してもデスクトップ上のガジェットは維持される。

---

## 7. 開発フェーズ計画

### Phase 1: MVP（最小実用構成）
- **M0（最優先スパイク検証・判断ゲート）**: 新規 Tauri v2 + 独立 `user_data_dir` の WebView2 内で **`muse.ai` へのログインおよびスペース（Artifacts）の表示・操作が正常にできるか** を実機確認する。
  - *判断ゲート*: もし認証方式等で弾かれてログインできない場合は、MVP 本実装へ進む前に「公開 URL 限定モードにするか」「既定ブラウザ経由認証／`Muse Desktop Mascot` 統合案へ切り替えるか」を再検討する。
- Tauri v2 プロジェクト初期構築（`DesktopDashboards`）
- `single-instance` ＋ CLI 引数（`--add-url`, `--manager`, `--silent`）
- 最背面固定モード（`HWND_BOTTOM` ＋ `skip_taskbar` ＋ `WS_EX_TOOLWINDOW`）
- ナビゲーション制御（同一オリジン遷移許可、外部リンクは既定ブラウザ起動）
- 複数ガジェットウィンドウの生成・破棄・`config.json` 原子的永続化（URL + ローカル HTML）
- 基本的な管理画面（追加・削除・表示切替・位置リセット）＋ トレイメニュー（自動起動 `--silent` 含む）

### Phase 2: ガジェット体験の磨き込み
- ロック機能（ホバー時のみ現れるドラッグハンドルバー＋リロード／ホームボタン）
- `WS_EX_LAYERED` による透明度・クリック透過（`WS_EX_TRANSPARENT`）・`WS_EX_NOACTIVATE`
- ズーム倍率・スクロールバー非表示・定期自動リロード（入力保護付き）
- マルチモニタ DPI スケール対応＆異常座標ガード

### Phase 3: 高度な固定と連携
- `WorkerW` 壁紙埋め込みモード（`Win + D` 耐性）とモード間座標変換
  - Explorer の再起動や壁紙スライドショー切替で `WorkerW` が再生成された際の**自動再アタッチ**（`TaskbarCreated` 検知等）も併せて検証する。
- カスタム CSS 注入機能
- `Muse Desktop Mascot` 側へのバンドル＆キック機能組み込み

---

## 8. Phase 4: ローカル HTML ブリッジ・宣言的コネクタ・複製可能ウィジェット（実装済み）

### 8.1 ローカル HTML 配信と作成時の技術仕様
- **カスタムプロトコル配信**: ローカル HTML ガジェット（`SourceType::File`）は `widgetfile` カスタムプロトコル（オリジン `http://widgetfile.localhost/{widget_id}/...`）経由で配信される。
- **相対パス資産の読み込み**: 選択した HTML と同じフォルダおよびサブフォルダ内の CSS / JS / 画像 / JSON 等は相対パスで読み込める（親ディレクトリ外へのパス参照は遮断）。
- **上端余白**: ロック解除時は上端 28px にホバー操作バー（ドラッグ移動・タイトル・複製・ホーム・リロード・ロック・閉じる）が表示されるため、ウィジェット側 `body` に `padding-top: 32px` 程度を確保することが推奨される。
- **外部リンク**: `http://` / `https://` へのリンク遷移やスクリプトからの `DD.openUrl(url)` 呼び出しは、ガジェット内ではなく OS の既定ブラウザで開く。

### 8.2 `window.DD` ブリッジ API と多層防御

ローカル HTML ガジェットのトップフレームには、以下の読み取り専用ブリッジ `window.DD` が注入される（`Object.freeze` / `writable: false` / `configurable: false`）。

| API | 用途・保存先 / 挙動 |
| :--- | :--- |
| `await DD.storage.get(key)` | ウィジェット単位の永続値を取得（キー未存在時は `null`）。 |
| `await DD.storage.set(key, value)` | ウィジェット単位の永続値を保存（`null` / `undefined` で削除）。保存先は `%APPDATA%\DesktopDashboards\storage\{widget_id}.json`。 |
| `await DD.config.get()` | 管理画面で保存したウィジェット設定（`%APPDATA%\DesktopDashboards\widget_config\{widget_id}.json`）を取得。 |
| `await DD.connect.request({ connection, method, path, body })` | 宣言的コネクタ経由で外部 API をプロキシ実行。`GET` / `HEAD` 以外のミューテーション実行時は `inFlightMutations` を加算し、自動リロードガードで書き込み中の再読込を延期する。 |
| `await DD.openUrl(url)` | `http://` または `https://` のみ許可して OS 既定ブラウザで開く（危険なスキームは拒否）。 |

#### 多層防御（ACL・フレーム境界・Origin 検証）
1. **初期化スクリプト制限**: `window === window.top` かつ `location.origin === 'http://widgetfile.localhost'` のときのみ `window.DD` を定義し、外部 `<iframe>` 等のサブフレームへ露出させない。
2. **Tauri v2 Capability 分離**:
   - `src-tauri/capabilities/widgets-dd.json`: `windows: ["widget-*"]`、`local: true`（`remote` 未定義）として `allow-dd-*` 権限をローカルカスタムプロトコル起点のみに限定。
   - `src-tauri/capabilities/widgets.json`: URL ガジェット向け（`http://*`, `https://*`）。`allow-dd-*` 権限は一切付与しない。
   - `src-tauri/capabilities/default.json`: 管理画面（`manager` ウィンドウ）専用の管理・接続・設定コマンドを分離。
3. **Rust 側コマンド強制検証**: `dd_*` 全コマンドで、呼び出し元ウィンドウの `SourceType::File` と IPC リクエストの `Origin` ヘッダ（`http://widgetfile.localhost`）を都度検証する。

### 8.3 宣言的コネクタエンジン・manifest 権限積集合・制限モード
- **コネクタ定義（`connectors/*.json`）**:
  - アプリ同梱コネクタ（`connectors/notion.json`、`Notion-Version: 2025-09-03` など）およびユーザー配置コネクタ（`%APPDATA%\DesktopDashboards\connectors\*.json`）を読み込む。
  - `base_url`、`allowed_hosts`、`auth`、`headers`、`credential_schema`、`test`、`endpoints` を宣言的に定義する。
- **ウィジェット manifest の探索優先順位**:
  - HTML と同じフォルダ内の `<html_stem>.manifest.json` を優先し、無ければ同フォルダの `manifest.json` を読み込む。
- **権限積集合と制限モード**:
  - `DD.connect.request` は **コネクタ定義の `endpoints`** と **ウィジェット manifest の `connections[].endpoints`** の双方に一致し、かつ `allowed_hosts` ガードを通過するリクエストのみ許可する。
  - manifest が存在しないローカル HTML は**制限モード**となり、`DD.storage` のみ利用可能で `DD.connect` は拒否される。

### 8.4 ガジェット設定フォーム自動生成（`DD.config`）
- ウィジェット manifest に `config_schema`（`string` / `boolean` 型）を定義すると、管理画面のガジェットカード内に **「⚙ ウィジェット設定（`DD.config`）」** フォームが自動生成される。
- 保存先は `%APPDATA%\DesktopDashboards\widget_config\{widget_id}.json` で、保存時に該当ガジェットが自動リロードされ、`await DD.config.get()` から最新値（未保存の任意項目は規定値・空文字）を取得できる。

### 8.5 外部サービス接続管理 UI と Windows 資格情報マネージャー連携
- 管理画面の「外部サービス接続（SaaS 連携）」パネルから、公式マークおよび送信先（`base_url` / `allowed_hosts`）を確認しながら接続の追加・トークン差し替え・削除・接続テスト・設定ガイド表示を行える。
- API トークン等の秘密情報は **Windows 資格情報マネージャー（Generic Credential）** にターゲット名 `DesktopDashboards:connection:<connection_id>:<credential_key>` で暗号化保管される。
- `%APPDATA%\DesktopDashboards\connections.json`（v2 形式）には秘密を含まないメタ情報のみを保持する（旧 v1 形式の平文トークンが残存する場合は起動時に資格情報マネージャーへ自動移行・消去する）。

### 8.6 複製可能ウィジェット（`"duplicable": true`）と `localStorage` 名前空間分離
- **複製可能ウィジェット（複数インスタンス追加）**:
  - ウィジェット manifest に `"duplicable": true` を宣言すると、ガジェット上端ホバーバーの `＋` ボタンおよび管理画面カードの `＋ 増やす` ボタンが表示される（未宣言時は非表示、URL ガジェットは対象外）。
  - 実行時は同一 HTML を参照する空の新規インスタンスを生成する（連番タイトル付与、カスケード配置、`DD.storage` / `widget_config` / `localStorage` は初期状態、`locked` 解除）。
- **`localStorage` のインスタンス分離と Lazy Adopt**:
  - ローカル HTML ガジェット（`SourceType::File`）のトップフレームでは、ホスト側初期化スクリプトのシムが `widget_id` ごとに `localStorage` キーの名前空間を自動分離する。これにより `localStorage` を使用する無改造の HTML でも複数枚並べて独立したデータを保持できる。
  - 既存データの非破壊引き継ぎ（Lazy Adopt）：分離導入前から存在するレガシーガジェット（`local_storage_isolated: false`）は、初回アクセス時に従来の共有キー値を自身の `widget_id` 名前空間へ非破壊で取り込んだ上で利用を継続する。新規追加・複製・初回サンプル生成時は `local_storage_isolated: true` で開始する。

### 8.7 配布形式および開発・検証用サンプル
- **配布パッケージ**:
  - 日本語・英語対応の Per-user NSIS インストーラ（`DesktopDashboards_x64-setup.exe`）およびポータブル ZIP アーカイブ（`DesktopDashboards-windows-x64.zip`）の両方に `samples/` および `connectors/` を同梱する。
- **開発・検証用サンプル**:
  - `samples/notion-spike.html`（＋`samples/notion-spike.manifest.json`）：Notion API（`2025-09-03`）および `window.DD` ブリッジ（`DD.storage` / `DD.connect.request` / manifest 権限積集合・資格情報マネージャー連携）の動作確認・開発検証用サンプル。
