# ジョブの文脈（会話・依頼・スレッド）

- 対象: 段階1
- 最終更新: 2026-10-09
- 前提として読む文書: [データソース仕様](data-source.md)、[Claude Code の会話記録](claude-transcripts.md)、[監視コア](monitor-core.md)
- 関連: [ADR-0004](../adr/0004-job-context-and-history.md)、[履歴](history.md)

ジョブが「どの会話から」「何のために」依頼され、「どのジョブの続き」なのかを求める部分。監視コア（`monitor-core`）の `context.rs` に置く。

背景: Codex のジョブは、同じワークスペースでも別のワークスペースでも複数が同時に動き、同じ会話から続けて依頼されたり、並行したサブエージェントから依頼されたりする。ワークスペース名と依頼文の先頭だけでは、どのタスクのジョブかがわかりにくい。

## 1. 求めるもの

```
JobContext {
  sessionId: string
  sessionTitle: string | null       会話のタイトル
  requester: Requester | null       依頼したもの（わからなければ null）
}
Requester {
  kind: "subagent" | "main"         サブエージェントか、会話の本体か
  description: string | null        依頼の説明（サブエージェントの description）
  agentType: string | null
  depth: number                     入れ子の深さ（本体は 0）
  parentDescription: string | null  depth が 2 以上のとき、親のサブエージェントの description
}
```

あわせて、ジョブごとに次を求める。

- `previousJobId`: 同じ `threadId` のジョブのうち、`createdAt` がこのジョブより前で最も新しいもの（履歴を含めて探す）。`--resume` で続きを頼むと同じスレッドが使われるため、これで「地続き」がわかる。

## 2. 会話の記録の探し方

1. `claude_projects_dir`（既定 `%USERPROFILE%\.claude\projects`）の直下の各フォルダに `<sessionId>.jsonl` があるかを見る。見つかったフォルダは `sessionId` ごとに覚えておく。
2. 見つからなければ、文脈は `{ sessionId, sessionTitle: null, requester: null }` とする（会話の記録が消えた、別のPCの会話、など）。

## 3. 会話のタイトル

1. `<sessionId>/custom-title.json` の `customTitle`。
2. 無ければ、本体の記録の**末尾から最大1MB**を読み、最後の `custom-title` の行の `customTitle`。
3. どちらも無ければ `null`（画面では「名前のない会話」と出す）。

タイトルは後から変わることがある。ファイルの更新時刻が変わったら読み直す（定期スキャンのついでに更新時刻だけ確かめる）。

## 4. 依頼したものの照合

ジョブ（`id`、`sessionId`、`createdAt`、依頼文）ごとに、次の順で探す。依頼文は、ジョブファイルの `request.prompt`、無ければジョブファイル側の `summary` を使う。

1. **ジョブIDによる照合**: `<sessionId>/subagents/agent-*.jsonl` のうち、更新時刻が `createdAt` の5分前より後のものを読み、ジョブIDの文字列を含むものを探す。見つかればそのサブエージェント。
2. **呼び出しによる照合**: 同じファイル群から、`tool_use` で `name` が `Bash`、`input.command` に `codex-companion.mjs` を含み、時刻が `[createdAt − 60秒, createdAt + 5秒]` に入るものを探す。そのうち、`input.command` に依頼文の先頭40文字（空白をまとめたもの）を含むものを採る。複数あれば `createdAt` に最も近いもの。
   - 依頼文が無い、または一致するものが無く、時間の範囲に入る候補が**ちょうど1件**なら、それを採る。
3. 1・2で見つからなければ、本体の記録（末尾から最大4MB）で2と同じ照合を行う。見つかれば `kind: "main"`、`depth: 0`、`description: null`。
4. どれでも見つからなければ `requester: null`。

サブエージェントが見つかったら、`agent-<agentId>.meta.json` から `description` / `agentType` / `spawnDepth` を取る。`spawnDepth` が2以上なら、ほかのサブエージェントの記録から `tool_use` の `id` が `toolUseId` と一致するものを探し、その `meta.json` の `description` を `parentDescription` にする。

## 5. 照合するタイミング

- ジョブが初めて現れたとき。
- 見つからなかったジョブは、定期スキャンのたびに（5秒ごと）照合し直す。ジョブが終わってから10分たっても見つからなければ、照合をやめる。
- 見つかった文脈は[履歴](history.md)に保存し、以後は会話の記録を読まない（記録が消えても文脈は残る）。会話のタイトルだけは、記録がある間は3のとおり更新する。

## 6. 負荷と安全

- 会話の記録は大きい（数MB以上）ことがある。照合は監視のスレッドとは別のスレッドで行い、監視（状態ファイルの読み込みとイベント）を待たせない。
- 1回の照合で読むのは、更新時刻で絞ったファイルだけとする。ジョブIDの文字列検索を先に行い、JSONとして解析するのは2の照合のときだけとする。
- 取り出すのは[会話記録の仕様 5](claude-transcripts.md#5-中身の扱い)の項目だけ。会話の本文やコマンドの中身は保存しない、表示しない、ログに出さない。
- 会話の記録が見つからない・読めない・形式が違う、はどれもエラーにせず、文脈を `null` にする（`tracing` には件数だけを記録）。

## 7. テスト

- 架空の会話記録（`custom-title.json`、本体の記録、サブエージェントの記録と `meta.json`）を一時ディレクトリに作って試す。最低限:
  - バックグラウンド実行（結果にジョブIDがある）
  - フォアグラウンド実行（ジョブIDが無く、呼び出しの時刻と依頼文で照合）
  - **並行した2つのサブエージェントが、同じワークスペースでほぼ同時にジョブを作った場合**に、依頼文でそれぞれ正しく照合されること
  - 入れ子（`spawnDepth` 2）で `parentDescription` が取れること
  - 本体から直接呼んだ場合（`kind: "main"`）
  - タイトルが `custom-title.json` に無く本体の記録にだけある場合、タイトルが変わった場合、タイトルが無い場合
  - 会話の記録が無い・壊れている場合
  - 同じ `threadId` の3つのジョブで `previousJobId` が順につながること
