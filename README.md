# MonitoringAI

Claude Code から [codex-plugin-cc](https://github.com/openai/codex-plugin-cc) 経由で Codex に作業を任せているとき、**Codex がいま何をしているか**を見えるようにする、Windows のトレイ常駐アプリです。

> **開発中です。** 段階1（監視）を実装し、受け入れ確認を進めているところです。

## できること（段階1）

- **トレイで状況がわかる**: 実行中の件数、完了、失敗をアイコンの色と数字で表示します。メニューから実行中のジョブを選べます。
- **通知**: Codex のジョブが完了・失敗したとき、止まった可能性があるときに Windows の通知を出します。
- **ジョブの一覧と詳細**: ジョブを「Claude の会話 → 依頼 → 地続きの前回」の階層で表示します。同じリポジトリで複数のジョブが同時に動いていても、依頼の説明で見分けられます。
- **リアルタイムのログ**: Codex が実行しているコマンドや発言を、追記されるそばから表示します。
- **履歴**: Claude の会話が終わると codex-plugin-cc はジョブの記録を削除しますが、本アプリは概要と結果を独自に残します（既定30日）。

## 今後の予定

- **段階2: フォーカスガード** — Codex が開発中のアプリを起動してウィンドウが手前に出ても、作業中のウィンドウにフォーカスを戻します。
- **段階3: 起動の窓口（MCP）** — Codex がアプリを安全に起動・確認できるツールを、MCP サーバーとして提供します。

詳しくは [設計書](docs/DESIGN.md) を参照してください。

## 仕組みと注意

- codex-plugin-cc の状態ファイル（`%USERPROFILE%\.claude\plugins\data\codex-openai-codex\state\`）を**読み取り専用**で監視します。プラグインや Codex の動作には介入しません。
- ジョブの文脈（会話のタイトルと依頼の説明）を得るため、Claude Code の会話記録（`%USERPROFILE%\.claude\projects\`）を読みます。**取り出して保存・表示するのは、会話のタイトルとサブエージェントの説明だけ**です。会話の本文は保存・表示・記録しません。
- どちらも**非公開の内部形式**に依存しています。codex-plugin-cc や Claude Code の更新で動かなくなる可能性があります（確認済み: codex-plugin-cc 1.0.6）。
- ネットワークは使いません。
- 本アプリは個人のツールであり、OpenAI・Anthropic とは関係ありません。

## 動作環境

- Windows 11（WebView2 ランタイムが入っていること。標準で入っています）
- Claude Code（Desktop 版を含む）と codex-plugin-cc 1.0.x

## 開発

必要なもの: Rust（stable）、Node.js 24 以上。

```bash
npm install
npm run tauri dev
```

- 開発用サーバーは `http://localhost:1430` を使います（Tauri の既定の 1420 番とぶつからないようにしています）。
- Rust 側のテスト: `cargo test --manifest-path src-tauri/Cargo.toml --workspace`
- 画面側のテスト: `npm test`
- TypeScript の型は Rust 側から生成します: `npm run bindings`（生成物は `src/bindings/` にあり、リポジトリに含めています）

構成:

```
src/                          画面（React + TypeScript + Vite）
src-tauri/src/                Tauri 本体（常駐、トレイ、通知、設定、コマンド）
src-tauri/crates/monitor-core 監視コア（状態ファイルの監視、文脈の照合、履歴。Tauri に依存しない）
docs/                         設計書と技術選定の記録（ADR）
```

## ライセンス

[MIT](LICENSE)
