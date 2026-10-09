# データソース仕様（codex-plugin-cc の状態ファイル）

- 対象: 全段階（全機能がこの文書を参照する）
- 最終更新: 2026-10-09
- 関連: [ADR-0002](../adr/0002-data-source.md)（この情報源を選んだ理由）

codex-plugin-cc **v1.0.6**（`gitCommitSha: db52e28f`）のソースと、手元の実ファイルで確認した内容をまとめる。**どれも非公開の内部形式**なので、変わる前提で読む（[8. 互換性の扱い](#8-互換性の扱い)）。

この文書は「外部の仕様をどう観測したか」の記録であり、本アプリの振る舞いは書かない。振る舞いは各機能の文書に書く。

## 1. 状態ルートの場所

プラグインは次の順で状態ルートを決める（`scripts/lib/state.mjs` の `resolveStateDir`）。本アプリは**両方を監視する**。

1. 環境変数 `CLAUDE_PLUGIN_DATA` がある場合: `%CLAUDE_PLUGIN_DATA%\state`
   - 通常は `%USERPROFILE%\.claude\plugins\data\codex-openai-codex\state`
2. ない場合: `%TEMP%\codex-companion`

状態ディレクトリ名は `<slug>-<hash>`。
- `slug` はワークスペースのフォルダ名から `[^a-zA-Z0-9._-]` を `-` に置き換えたもの
- `hash` は実パス（`realpath`）のSHA-256の先頭16桁

**表示に使うワークスペース名は、ディレクトリ名ではなくジョブの `workspaceRoot` から取る。**

## 2. ディレクトリ構成

```
<状態ディレクトリ>/
├─ state.json            ワークスペースの全ジョブの一覧（最大50件）と設定
├─ broker.json           ブローカーの情報（ブローカーが動いていないときは無いことがある）
└─ jobs/
   ├─ <jobId>.json       ジョブ1件の詳細
   └─ <jobId>.log        ジョブの進行ログ
```

## 3. ジョブのフィールド

`state.json` の `jobs[]` と `jobs/<jobId>.json` は、ほぼ同じ形をしている。違いは表の右端の列のとおり。

| フィールド | 型 | 意味 | 備考 |
|---|---|---|---|
| `id` | string | ジョブID（`task-xxxxxxxx-xxxxxx` の形式） | **必須** |
| `status` | string | `queued` / `running` / `completed` / `failed` / `cancelled` | **必須** |
| `phase` | string \| null | [4. phase の値](#4-phase-の値) | |
| `kind` | string | `task` / `review` / `adversarial-review` など | |
| `kindLabel` | string | `rescue` / `review` / `adversarial-review` | 一覧の種別表示に使う |
| `jobClass` | string | `task` / `review` | |
| `title` | string | `Codex Task` / `Codex Resume` / `Codex Stop Gate Review` など | |
| `summary` | string | 依頼内容の先頭を短くしたもの | **`state.json` 側は完了時に結果の要約で上書きされる**。ジョブファイル側は依頼内容のまま |
| `workspaceRoot` | string | ワークスペースの絶対パス | `/` 区切りのことがある |
| `write` | bool | ファイルの書き込みを許可したか | |
| `sessionId` | string? | 依頼元のClaudeセッションのID | 無いことがある |
| `pid` | number \| null | [6. プロセスの構造](#6-プロセスの構造) | 終了すると `null` |
| `threadId` / `turnId` | string? | CodexのスレッドID・ターンID | 開始直後は無い |
| `logFile` | string | ログファイルの絶対パス | |
| `createdAt` | ISO8601 | 作成時刻 | |
| `updatedAt` | ISO8601 | 最終更新時刻 | **`state.json` 側だけ**にある。フェーズなどが変わったときだけ更新される |
| `startedAt` / `completedAt` / `cancelledAt` | ISO8601? | 開始・終了・キャンセルの時刻 | |
| `request` | object? | `{ cwd, model, effort, prompt, write, resumeLast, jobId }` | **バックグラウンド実行のときだけ**ある。`prompt` が依頼の全文 |
| `result` / `rendered` | any / string | 完了時の結果 | **ジョブファイル側だけ** |
| `errorMessage` | string? | 失敗・キャンセルの理由 | |

**一覧には `state.json` を使い、詳細表示ではジョブファイルを読み足す。** 依頼の全文は `request.prompt` から取る。これが無いとき（フォアグラウンド実行）は、ジョブファイル側の `summary` を使う。

## 4. `phase` の値

| 値 | 意味 | 表示（案） |
|---|---|---|
| `queued` | バックグラウンド実行の待ち | 待機中 |
| `starting` | スレッドを準備中 | 開始中 |
| `investigating` | 調べている（検索、ツール呼び出し、ファイル読み込み） | 調査中 |
| `running` | コマンドを実行中 | コマンド実行中 |
| `verifying` | テスト・ビルド・lintなどを実行中 | 検証中 |
| `editing` | ファイルを編集中 | 編集中 |
| `reviewing` | レビュー中 | レビュー中 |
| `finalizing` | 最終回答を作成中 | まとめ中 |
| `done` / `failed` / `cancelled` | 終了 | 完了 / 失敗 / キャンセル |

知らない値はそのまま表示する。

## 5. 書き込みの仕方

- **ジョブの作成時と進捗イベントのたびに、`state.json` 全体とジョブファイル全体を書き直す。** 書き込みは `writeFileSync` で直接行い、一時ファイルを経由しない。読むタイミングによっては、**空のファイルや途中までのJSONを読む**ことがある。
- 進捗イベントでの更新は、`phase` / `threadId` / `turnId` のどれかが変わったときだけ行う。つまり**同じフェーズのまま長いコマンドを実行している間は、`state.json` もジョブファイルも更新されない**。一方、ログはイベントのたびに追記される。
- フォアグラウンド実行でもバックグラウンド実行でも、同じ仕組み（`runTrackedJob`）で書き出される。
- ジョブが51件以上になると、`updatedAt` の古いものから `state.json` から外され、**そのジョブファイルとログファイルは削除される**。
- キャンセル（`/codex:cancel`）では `status` と `phase` が `cancelled` になり、`cancelledAt` が付く。
- **Claude の会話が終わると（SessionEnd フックの `cleanupSessionJobs`）、その会話の `sessionId` を持つジョブを `state.json` から外し、ジョブファイルとログファイルも削除する。** まだ `queued` / `running` のジョブは、プロセスごと強制終了する。会話を閉じる・アプリを再起動するなどで起こる（2026-10-09に、この会話の前半のジョブが消えていたことで確認）。本アプリはこれに備えて独自の[履歴](history.md)を持つ。

## 6. プロセスの構造

```
Claude Code
 └─ node codex-companion.mjs  … ジョブの pid
     （フォアグラウンド実行なら Claude の配下。バックグラウンドなら切り離されたワーカー）
        │ 名前付きパイプで接続
        ▼
node app-server-broker.mjs     … broker.json の pid（ワークスペースごとに1つ。切り離されて常駐）
 └─ (cmd.exe) └─ codex.exe app-server
                   └─ Codexが実行するコマンド（シェル、npm、cargo、起動したアプリ…）
```

- ジョブの `pid` は、`queued` のときは切り離されたワーカー、`running` のときは companion のプロセスを指す。終了すると `null`。
- ブローカーが別のジョブで使用中のときは、companion が自分の配下で `codex app-server` を直接起動する。このときCodexが実行するコマンドは**ジョブの `pid` の配下**にぶら下がる。
- ブローカーは、ClaudeのセッションがSessionEndフックで終了するときに片付けられる。

`broker.json` の形:

```json
{ "endpoint": "pipe:\\\\.\\pipe\\cxc-XXXX-codex-app-server", "pid": 48064,
  "pidFile": "...\\Temp\\cxc-XXXX\\broker.pid", "logFile": "...\\broker.log", "sessionDir": "...\\Temp\\cxc-XXXX" }
```

## 7. ログファイルの形式

UTF-8で追記のみ。2つの形式がある。

```
[2026-10-02T19:51:45.333Z] Thread ready (01a0fe2c-...).          ← 1行の形式
                                                                  ← ブロックの前は空行
[2026-10-02T19:51:50.121Z] Assistant message                      ← ブロックの見出し
指定の設計節と既存実装を確認してから編集します。…                  ← 本文（複数行）
```

主なメッセージ: `Starting Codex Task.` / `Queued for background execution.` / `Thread ready (<threadId>).` / `Turn started (<turnId>).` / `Running command: …` / `Command completed: … (exit N)` / `Command failed: … (exit N)` / `Applying N file change(s).` / `Cancelled by user.`。主なブロック: `Assistant message` / `Final output`。

ログは、同じジョブIDで空のファイルから作り直されることがある。51件以上になると削除される。

## 8. 互換性の扱い

- 今のプラグインのバージョンは `%USERPROFILE%\.claude\plugins\installed_plugins.json` の `plugins["codex@openai-codex"][0].version` から読む。
- **動作を確認したバージョン**（最初は `1.0.x`）以外では、画面に「未確認のバージョンです」と表示する。それでも読めるものは表示する。
- JSONは `id` と `status` だけを必須とし、他のフィールドは全部任意とする。知らないフィールドは無視し、知らない値はそのまま表示する。
- 状態ディレクトリの構造が見つからない、などの致命的な不一致は、トレイのメニューと画面にエラーとして出す。

プラグインを更新したら、この文書の内容をソースと照らし合わせ、確認済みのバージョンを更新する。

## 9. 補助データ（段階1では使わない）

- Codexの実行記録: `%USERPROFILE%\.codex\sessions\YYYY\MM\DD\rollout-<ローカル時刻>-<threadId>.jsonl`。**ファイル名の末尾がジョブの `threadId` と一致する**ことを確認済み。1行目の `session_meta` に `originator: "Claude Code"` と `cwd` がある。トークン数などの詳細表示を足すときに使う。
  - **このファイルにはアカウントの識別子が含まれる。** 使う場合も、必要なフィールドだけを取り出し、他の項目は表示・記録・保存しない。
