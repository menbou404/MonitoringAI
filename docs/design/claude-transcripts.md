# データソース仕様（Claude Code の会話記録）

- 対象: 段階1（ジョブの文脈）
- 最終更新: 2026-10-09
- 関連: [ADR-0004](../adr/0004-job-context-and-history.md)（この情報源を使う理由）、[ジョブの文脈](job-context.md)（この情報の使い方）

Claude Code（Desktop版を含む）が残す会話の記録のうち、本アプリが使う部分を、手元の実ファイル（Claude Code Desktop、2026-10-09時点）で確認した内容としてまとめる。**非公開の内部形式**なので、変わる前提で読む。

この文書は観測の記録であり、本アプリの振る舞いは[ジョブの文脈](job-context.md)に書く。

## 1. 置き場所

```
%USERPROFILE%\.claude\projects\
└─ <作業フォルダから作った名前>\          例: C--Users-GreenBird-WebProjects-MonitoringAI
   ├─ <sessionId>.jsonl                   会話の本体の記録（1行に1件のJSON）
   └─ <sessionId>\
      ├─ custom-title.json                会話のタイトル（付いている場合だけ）
      └─ subagents\
         ├─ agent-<agentId>.jsonl          サブエージェントの記録
         └─ agent-<agentId>.meta.json      サブエージェントの説明
```

- 作業フォルダから作った名前は、パスの英数字以外を `-` に置き換えたもの。ただし、worktree などで作業フォルダが変わると別の名前になる。**本アプリは名前を組み立てず、`projects` の下の全フォルダから `<sessionId>.jsonl` を探す。**
- `sessionId` は、codex-plugin-cc のジョブの `sessionId` と同じ値（プラグインが SessionStart フックで受け取った値。[データソース仕様 3](data-source.md#3-ジョブのフィールド)）。

## 2. 会話のタイトル

- `custom-title.json`: `{ "customTitle": "Codex実行状況の監視アプリ" }`
- 本体の記録の中にも `{"type":"custom-title","customTitle":"…","sessionId":"…"}` の行がある。タイトルを変えるたびに追記されるので、**最後の行が今のタイトル**。
- タイトルが無い会話もある。

## 3. サブエージェントの説明（`agent-<agentId>.meta.json`）

```json
{ "agentType": "codex:codex-rescue", "description": "Codexに画面の実装を依頼",
  "toolUseId": "toolu_…", "spawnDepth": 1, "requestShape": "background", "requestNonInteractive": true }
```

| フィールド | 意味 |
|---|---|
| `agentType` | サブエージェントの種類。codex-plugin-cc 経由の依頼では `codex:codex-rescue` |
| `description` | **依頼元（Claude）が書いた、その依頼の短い説明**。本アプリでは「依頼の説明」として表示する |
| `toolUseId` | このサブエージェントを起動した呼び出しのID。親（本体またはほかのサブエージェント）の記録の `tool_use` の `id` と一致する |
| `spawnDepth` | 入れ子の深さ。本体から起動したら 1、サブエージェントから起動したら 2 以上 |

## 4. 記録の行（`*.jsonl`）

1行に1件のJSON。本アプリが使うのは次の2種類だけ。

- `type: "assistant"` の行の `message.content[]` のうち `type: "tool_use"` のもの: `{ "name": "Bash", "input": { "command": "…" }, "id": "toolu_…" }`。行の `timestamp` が呼び出した時刻。
- `type: "user"` の行の `message.content[]` のうち `type: "tool_result"` のもの: 呼び出しの結果。

codex-plugin-cc へのジョブの依頼は、次のような Bash の呼び出しとして残る。

```
node "…/codex-companion.mjs" task "<依頼文>"            ← tool_use（ジョブ作成の約1秒前）
Codex Task started in the background as task-xxxxxxxx-xxxxxx. …   ← tool_result（バックグラウンド実行のとき）
```

- バックグラウンド実行では、結果に**ジョブID**が出る。フォアグラウンド実行では、結果に出るとは限らない。
- 手元の例では、`tool_use` の時刻（01:41:17.736）の0.85秒後にジョブが作られた（`createdAt` 01:41:18.587）。

## 5. 中身の扱い

会話の記録には、会話の全文、ファイルの内容、コマンドの出力などが含まれる。本アプリは、[ジョブの文脈](job-context.md)に書いた照合のためにだけ読み、**取り出して保存・表示するのは、会話のタイトル、サブエージェントの `description` / `agentType` / `spawnDepth` だけ**とする。それ以外の内容は、メモリ上の照合にだけ使い、ログにも出さない。
