# 履歴

- 対象: 段階1
- 最終更新: 2026-10-09
- 前提として読む文書: [データソース仕様 5](data-source.md#5-書き込みの仕方)、[ジョブの文脈](job-context.md)、[監視コア](monitor-core.md)
- 関連: [ADR-0004](../adr/0004-job-context-and-history.md)

codex-plugin-cc は、**Claude の会話が終わると、その会話のジョブを状態ファイルからログごと削除する**（[データソース仕様 5](data-source.md#5-書き込みの仕方)）。51件を超えたときも古いものから削除する。そのままでは、会話を閉じると一覧から履歴が消える。そこで、本アプリが独自にジョブの概要と結果を保存する。監視コアの `history.rs` に置く。

## 1. 保存するもの

1件のジョブにつき1ファイル: `<history_dir>/<jobId>.json`。`history_dir` は本アプリが渡す（既定 `app_local_data_dir()/history`、Windowsでは `%LOCALAPPDATA%\com.menbou404.monitoringai\history\`）。

```
{
  "version": 1,
  "id", "workspaceRoot", "kind", "kindLabel", "title", "write",
  "promptHead":   依頼文の先頭200文字,
  "summary":      state.json 側の summary（完了後は結果の要約）,
  "rendered":     最終出力（64KBで切る。切ったら truncated を真に）,
  "renderedTruncated": bool,
  "errorMessage",
  "status", "effectiveState",       最後に見た値
  "createdAt", "startedAt", "completedAt",
  "threadId", "sessionId",
  "context":      JobContext（ジョブの文脈 1）,
  "archivedNote": 一覧から消えたときの注記（2を参照）。無ければ null
}
```

- **ログは保存しない。** 依頼の全文も保存しない（先頭200文字だけ）。
- 書き込みは一時ファイルに書いてから置き換える。

## 2. 保存するタイミング

| きっかけ | 内容 |
|---|---|
| ジョブが初めて現れた | 新しく作る（`promptHead` などその時点で分かるもの） |
| 文脈が求まった | `context` を更新する |
| 実効状態が `Completed` / `Failed` / `Cancelled` になった | `summary` / `rendered` / `errorMessage` / `completedAt` などを更新する |
| 状態ファイルから消えた | 消えた時点の実効状態が `Queued` / `Running` / `Quiet` / `Orphaned` なら、`effectiveState` を `Cancelled` にし、`archivedNote` を「codex-plugin-cc の一覧から消えました。Claude の会話の終了などで中断された可能性があります。」にする。終わっていたジョブはそのまま |

フェーズの変化や最新のメッセージの変化では書かない（書き込みを減らすため）。

## 3. 一覧への合成

- 監視コアのスナップショットは、**状態ファイルにあるジョブ（`archived: false`）と、履歴にだけあるジョブ（`archived: true`）を合わせたもの**とする。
- 状態ファイルから消えたジョブは、`JobsChanged` の `removed` ではなく、`archived: true` にした `changed` として出す。`removed` に出すのは、履歴からも消したときだけ。
- 状態ファイルから消えたときに `Cancelled` にしたジョブは、`StateTransition`（`to: Cancelled`）を出す。
- 履歴にだけあるジョブの `job_detail` は、履歴から作る（`prompt` は `promptHead`、`rendered` / `errorMessage` は履歴の値）。`read_log` は `None` を返す。

## 4. 保存期間

- 設定の「履歴の保存期間」（既定30日、1〜90日）を過ぎたもの（`completedAt`、無ければ `createdAt` で判断）を削除する。
- 起動時と、その後は1時間ごとに整理する。状態ファイルにまだあるジョブは削除しない。

## 5. 起動時

- `history_dir` の全ファイルを読み、状態ファイルに無いものを `archived: true` として一覧に入れる。
- 読めないファイルは無視し、件数だけを `tracing` に記録する。
- 起動時に状態ファイルにある・履歴に無いジョブは、その場で履歴を作る（すでに終わっていても）。

## 6. テスト

- 一時ディレクトリで、作成・文脈の更新・完了時の更新・状態ファイルから消えたとき（実行中だった場合と、終わっていた場合）・保存期間の整理・再起動後の読み込み・壊れたファイル・`rendered` の64KBでの切り詰め（マルチバイト文字の途中で切らない）を試す。
- 結合テスト: 状態ファイルからジョブを消したときに、`removed` ではなく `archived: true` の `changed` が出ること。
