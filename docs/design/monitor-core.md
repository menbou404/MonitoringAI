# 監視コア（monitor-core）

- 対象: 段階1
- 最終更新: 2026-10-09
- 前提として読む文書: [データソース仕様](data-source.md)

状態ファイルを見張り、ジョブのスナップショットと変更イベントを作る部分。Tauriに依存しないクレート `src-tauri/crates/monitor-core` として作る。トレイ・通知・画面は、ここが出すスナップショットと変更イベントだけを使う。

## 1. モジュール構成

```
monitor-core/
├─ src/
│  ├─ discovery.rs   状態ルートと状態ディレクトリの列挙
│  ├─ watcher.rs     ファイル変更の監視と定期スキャン
│  ├─ parser.rs      state.json / ジョブファイル / broker.json / installed_plugins.json の読み込み
│  ├─ model.rs       Job, JobStatus, Phase, EffectiveState など
│  ├─ liveness.rs    PIDの生存確認（トレイトで差し替えられるようにする）
│  ├─ log_tail.rs    ログの差分読み込み
│  ├─ store.rs       スナップショットの保持、実効状態の判定、変更の検出
│  ├─ context.rs     ジョブの文脈（会話・依頼・スレッド）の照合（[ジョブの文脈](job-context.md)）
│  └─ history.rs     独自の履歴の保存と読み込み（[履歴](history.md)）
├─ examples/
│  └─ watch.rs       手動確認用。実際の状態ルートを監視してイベントを標準出力に出す
└─ tests/            fixtureを使った結合テスト
```

依存は [Cargo.toml](../../src-tauri/crates/monitor-core/Cargo.toml) にあるものだけを使う: `notify` + `notify-debouncer-full`、`serde` / `serde_json`、`chrono`、`thiserror`、`tracing`、`windows`（Windowsのみ）、`ts-rs`（`ts` 機能のときだけ）、`tempfile`（テストのみ）。

**Windows固有の処理は `liveness.rs` だけに置く。** 他のモジュールはOSに依存させない。

### 1.1 実装の取り決め

- **同期処理（スレッド）で作る。** async ランタイムに依存しない。Tauri側は tokio を使うが、監視コアはスレッドとチャンネル（`std::sync::mpsc`）で完結させ、Tauri側で受け取って橋渡しする。`Monitor` を drop したら、内部のスレッドは全部止まる。
- **時刻**は `chrono::DateTime<Utc>` で持つ。解析できない時刻は `None` として扱い、ジョブ自体は捨てない。
- **テストのための差し替え口**を2つ設ける。
  - `ProcessProbe` トレイト: `pid` から「プロセスの作成時刻」と「実行ファイル名」を返す（存在しなければ `None`）。Windowsでは `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `GetProcessTimes` + `GetExitCodeProcess` + `QueryFullProcessImageNameW` で実装する。Windows以外では「判定できない」を返し、生きているものとして扱う。
  - `Clock` トレイト: 現在時刻を返す。`Quiet` の判定と経過時間のテストで使う。
- **設定**（`MonitorConfig`）の項目と既定値:

  | 項目 | 既定値 |
  |---|---|
  | `state_roots` | [データソース仕様 1](data-source.md#1-状態ルートの場所)の場所。`%USERPROFILE%` の下の既定の場所と `%TEMP%codex-companion` は**常に**含め、`CLAUDE_PLUGIN_DATA` があればその下も加える（重複は除く）。`default_state_roots()` で求める |
  | `extra_state_roots` | なし |
  | `installed_plugins_path` | `%USERPROFILE%\.claude\plugins\installed_plugins.json` |
  | `supported_plugin_versions` | `1.0.` で始まるもの |
  | `quiet_after` | 10分 |
  | `rescan_interval` / `liveness_interval` | 5秒 / 5秒 |
  | `debounce` | 200ms |
  | `initial_log_bytes` | 1MB |
  | `claude_projects_dir` | `%USERPROFILE%.claudeprojects`（`None` なら文脈を求めない） |
  | `history_dir` | `None`（本アプリが `app_local_data_dir()/history` を渡す。`None` なら履歴を使わない） |
  | `history_days` | 30 |

- **型**: 画面に渡す型（`JobSummary`、`JobDetail`、`EffectiveState`、`LogChunk`、`Health` など）は `serde::Serialize` を付け、`ts` 機能のときだけ `#[cfg_attr(feature = "ts", derive(ts_rs::TS))]` を付ける。フィールド名は camelCase で出す。
- **`JobSummary` に最低限持たせる項目**: `id`、`workspaceRoot`、`workspaceName`（`workspaceRoot` の末尾のフォルダ名）、`kind`、`kindLabel`、`title`、`summary`（`state.json` 側）、`status` と `phase`（生の文字列）、`effectiveState`、`createdAt` / `startedAt` / `completedAt` / `updatedAt`、`pid`、`threadId`、`write`、`sessionId`、`latestMessage`、`logModifiedAt`。
- **文脈と履歴のための `JobSummary` の項目**: `context`（`JobContext` または `null`。[ジョブの文脈 1](job-context.md#1-求めるもの)）、`previousJobId`、`archived`（状態ファイルから消え、履歴にだけある）、`archivedNote`、`promptHead`（依頼文の先頭200文字。一覧の表示用）。
- **`JobDetail` の追加項目**: `prompt`（依頼の全文。取り方は[データソース仕様 3](data-source.md#3-ジョブのフィールド)）、`turnId`、`model`、`effort`、`rendered`、`errorMessage`、`logFile`。
- **`latestMessage`**: ログの最後の1行の形式のメッセージ（ブロックなら見出し＋本文の1行目）。200文字で切る。
- **手動確認用の `examples/watch.rs`**: 既定の設定で監視を始め、`MonitorEvent` を1行ずつ標準出力に出す。出すのはID、ワークスペース名、`status`、`phase`、実効状態、`latestMessage` の先頭まで。依頼の全文は出さない。Ctrl+Cで終わる。

## 2. 監視の流れ

1. **列挙**: 起動時と定期スキャンのたびに、全状態ルート（[データソース仕様 1](data-source.md#1-状態ルートの場所)と、設定で追加したもの）の下の `*/state.json` を探す。
2. **変更の監視**: 状態ルートを再帰的に監視する（`notify`）。変更通知は200msまとめてから処理する。
   - **再帰的に監視してよいのは、状態ルートそのものだけ**とする。状態ルートがまだ無い場合は、存在する最も近い親ディレクトリを**再帰なしで**監視し、状態ルートが作られたら監視を張り直す。親（例: `%TEMP%`）を再帰的に監視すると、無関係な書き込みのたびに処理が走り、待機中もCPUを使い続けるため。
   - 変更通知を受けたら、たまった通知をまとめて1回だけ処理する。処理は「更新時刻とサイズが変わったファイルだけを読み直す」とし、全ファイルを強制的に読み直さない。
3. **定期スキャン**: 変更通知は取りこぼすことがあるので、**5秒ごとに**全 `state.json` の更新時刻とサイズを確かめ、変わっていれば読み直す。
4. **読み込み**:
   - ファイルは読み取り専用で開く。プラグイン側の書き込みや削除を妨げないよう、共有モードは読み・書き・削除のすべてを許可する（Rust標準の `File::open` の既定値がそうなっている）。
   - 空のファイルやJSONの解析エラーは「書き込み途中」とみなし、100ms間隔で3回まで読み直す。それでも駄目なら**前回読めた内容を使い続け**、警告を記録する（`tracing`）。
   - ファイルの読み込み（読み直しの待ちを含む）は、スナップショットのロックを持たずに行う。ロックは、読み終えた結果を反映する間だけ持つ。`snapshot()` などの問い合わせを待たせないため。
5. **生存確認**: 5秒ごとに、`queued` / `running` のジョブの `pid` を確かめる（[3. 実効状態の判定](#3-実効状態の判定)）。
6. **ログの差分読み込み**: [4. ログの差分読み込み](#4-ログの差分読み込み)。
7. **変更の検出**: 前回のスナップショットと比べ、変更イベントを出す（[5. 公開するインターフェース](#5-公開するインターフェース)）。

## 3. 実効状態の判定

プラグインが書く `status` だけでは「本当に動いているか」がわからない。Claudeが異常終了すると `running` のまま残るため。そこで次の実効状態を判定する。

| 実効状態 | 条件 | 表示 |
|---|---|---|
| `Queued` | `status = queued` かつ pid が生きている | 待機中 |
| `Running` | `status = running` かつ pid が生きている | 各フェーズの表示 |
| `Quiet` | `Running` の条件を満たし、かつ**ログの更新時刻**が N 分以上前（既定10分。設定で変えられる） | 「応答なし？（N分）」 |
| `Orphaned` | `status` が `queued` / `running` なのに pid が生きていない | 「停止した可能性」 |
| `Completed` / `Failed` / `Cancelled` | `status` がそれぞれの値 | 完了 / 失敗 / キャンセル |
| `Unknown` | 知らない `status` | 値をそのまま表示 |

- **PIDの使い回しに注意する。** pid が生きていても、そのプロセスの作成時刻が次の「基準時刻」より**1秒以上後**なら、別のプロセスとみなして `Orphaned` にする。作成時刻は `GetProcessTimes` で取る。実行ファイル名が `node.exe` であることも確かめる。
  - 基準時刻は、`running` なら `startedAt`、`queued` なら `updatedAt`（`state.json` 側）とする。無ければ判定を省く（生きているものとして扱う）。
  - **`createdAt` は基準にしない。** バックグラウンド実行では、プラグインはジョブを作って（`createdAt`）からワーカーのプロセスを起動するので、正常なジョブでもプロセスの作成時刻が `createdAt` より後になる。一方、`queued` の記録（`updatedAt`）はワーカーの起動後に、`running` の記録（`startedAt`）はワーカー自身が起動後に書く（[データソース仕様 6](data-source.md#6-プロセスの構造)）。
  - 1秒の余裕は、時刻の丸めの差を吸収するため。
- `Quiet` の判定には `updatedAt` を使わない。同じフェーズの間は更新されないため（[データソース仕様 5](data-source.md#5-書き込みの仕方)）。
- 経過時間は `startedAt`（無ければ `createdAt`）から数える。

## 4. ログの差分読み込み

- 対象は、購読中のジョブ（画面で詳細を表示中）と、`running` のジョブ（トレイのツールチップに最新メッセージを出すため）。
- 読むきっかけは、`jobs/*.log` の変更通知と、定期スキャン（取りこぼし対策）の2つ。
- ジョブごとに、読んだ位置（バイト）を覚えておき、差分だけを読む。
- ファイルが前回より小さくなっていたら、作り直されたとみなして先頭から読み直す。ファイルが消えたら、読んだ位置を捨てる。
- UTF-8のマルチバイト文字が読み込み単位の境目にかかったら、その手前で区切り、残りは次回に回す。
- 初回は末尾から1MBだけを読む。それより前は、要求されたときだけ読む。
- 1行の形式とブロックの形式（[データソース仕様 7](data-source.md#7-ログファイルの形式)）を解析し、「最新のメッセージ」（トレイ用）を取り出せるようにする。

## 5. 公開するインターフェース

Tauri側（[トレイと通知](tray-notification.md)、[画面](window-ui.md)）が使う。

| 名前 | 内容 |
|---|---|
| `Monitor::start(config) -> (Monitor, Receiver<MonitorEvent>)` | 監視を始める。`config` は状態ルート、`Quiet` までの分数など |
| `Monitor::snapshot()` | 全ジョブの `JobSummary`（実効状態を含む） |
| `Monitor::job_detail(id)` | ジョブファイルを読み足した `JobDetail` |
| `Monitor::read_log(id, from: Option<u64>)` | `{ text, start, end, truncatedBefore }`。`from` を省くと末尾から `initial_log_bytes` 分。`truncatedBefore` は「`start` より前にまだ読んでいない内容がある」（`start > 0`） |
| `Monitor::subscribe_log(id)` / `unsubscribe_log(id)` | ログの購読の開始・終了 |
| `Monitor::health()` | プラグインのバージョン、監視中の状態ルート、直近のエラー |
| `Monitor::update_config(config)` | 設定の変更を反映する |

`MonitorEvent` の種類:

| 種類 | 内容 |
|---|---|
| `JobsChanged { changed, removed }` | 追加・状態の変化・フェーズの変化・最新メッセージの変化・文脈が求まった・履歴に移った（`archived` になった）ジョブと、削除されたID。**状態ファイルから消えただけのジョブは `removed` ではなく `changed`（`archived: true`）で出す**（[履歴 3](history.md#3-一覧への合成)） |
| `StateTransition { id, from: Option<EffectiveState>, to }` | 実効状態の変化。通知の判定に使う。**監視を始めたときの最初の読み込みでは出さない**。それ以降に新しく現れたジョブは `from: None` で出す（現れた時点で `Running` のジョブや、スキャンの間に始まって終わったジョブも通知できるように） |
| `LogAppended { id, text, start, end }` | 購読中のジョブのログの追記 |
| `HealthChanged(health)` | バージョンの不一致や読み込みエラーなど |

## 6. テスト

- **fixture**: 実ファイルの構造を写した**架空のデータ**で作る（[DESIGN.md 6](../DESIGN.md#6-共通方針)）。最低限用意するケース:
  - バックグラウンド実行の `queued` → `running`（各フェーズ）→ `completed`
  - フォアグラウンド実行（`request` が無い）
  - `failed`（`errorMessage` あり）、`cancelled`
  - 知らないフィールドと知らない `phase` を含むもの、`sessionId` / `threadId` が無いもの
  - 空のファイル、途中で切れたJSON
- **単体テスト**: 読み込み、実効状態の判定（PIDの生存・使い回し・作成時刻はトレイトで差し替える）、変更の検出、ログの差分読み込み（追記、作り直し、削除、ブロック形式、マルチバイト文字の境目）。
- **結合テスト**: 一時ディレクトリを状態ルートにし、プラグインの書き込み（途中までの書き込み、51件以上による削除、状態ディレクトリの追加と削除）を再現して、スナップショットと変更イベントを確かめる。
  - **バックグラウンド実行の時刻の並び**（`createdAt` < プロセスの作成時刻 ≦ `updatedAt`（queued）/ `startedAt`（running））で `Orphaned` にならないこと。
  - 監視開始後に新しく現れたジョブで、`from: None` の `StateTransition` が出ること。
- **待機中の負荷**: 状態ルートが無い状態で、その親ディレクトリに無関係なファイルを書き続けても、状態ファイルの読み込みが定期スキャン以外で起きないこと。
