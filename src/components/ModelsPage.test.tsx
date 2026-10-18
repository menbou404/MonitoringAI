import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { App } from "../App";
import type { ModelInfo } from "../bindings/ModelInfo";
import type { ModelSwitchStatus } from "../bindings/ModelSwitchStatus";
import type { Settings } from "../bindings/Settings";
import { combinationError, modelName, normalizeCombination, resetTime, switchConfirmation } from "../model/models";
import { initialState, reducer } from "../model/store";
import { CombinationPicker, ManualSwitchConfirmation, ModelSection, ModelsPage } from "./ModelsPage";
import { SettingsDialog } from "./SettingsDialog";

const models: ModelInfo[] = ["base", "saver"].map((slug) => ({ slug, display_name: `表示名 ${slug}`, description: `説明 ${slug}`,
  default_reasoning_level: "low", supported_reasoning_levels: [{ effort: "low", description: "速く考える" }, { effort: "medium", description: "標準" }], visibility: "list", priority: 1 }));
const settings: Settings = { notifications: { enabled: true, started: false, completed: true, failed: true, orphaned: true, quiet: false },
  quiet_minutes: 10, list_days: 3, history_days: 30, runningFolderOrder: [], extra_state_roots: [], autostart: false, window: null,
  agents: { codex: { autoSwitch: true, threshold: 90, base: { model: "base", effort: null }, saver: { model: "saver", effort: "low" }, switchedByApp: true } } };
const now = new Date(2026, 9, 18, 10).getTime();
const status: ModelSwitchStatus = { usage: { fiveHour: { usedPercent: 92, resetsAt: new Date(2026, 9, 18, 14, 30).getTime() / 1000 }, week: null },
  currentModel: "saver", currentEffort: "medium", switchedByApp: true, models, error: null,
  history: [{ time: now / 1000, agent: "codex", trigger: "auto-saver", before: { model: "base", effort: "high" }, after: { model: "saver", effort: "medium" }, usage: null }] };
const noop = async () => {};

describe("model page", () => {
  it("shows usage resets, current display name and effort, controls and newest-first history", () => {
    const html = renderToStaticMarkup(<ModelsPage settings={settings} status={status} now={now} onSave={noop} onManual={noop} />);
    expect(html).toContain("14:30 にリセット");
    expect(html).toContain('class="usage-meter usage-high"');
    expect(html).toContain("記録なし");
    expect(html).toContain("表示名 saver · medium");
    expect(html).toContain("節約中");
    expect(html).toContain("config.toml");
    expect(html).toContain("今すぐ既定にする");
    expect(html).toContain("自動で節約用へ");
    expect(html).toContain("表示名 base · high → 表示名 saver · medium");
    expect(html).not.toContain("既定の変更を確認");
  });

  it("offers descriptions, only supported efforts and do-not-touch, and excludes the other model", () => {
    const html = renderToStaticMarkup(<CombinationPicker label="基本" value={{ model: "base", effort: "medium" }} models={models} excluded="saver" onChange={() => {}} />);
    expect(html).toContain("説明 base");
    expect(html).not.toContain("説明 saver");
    expect(html).not.toContain('type="radio"');
    expect(html).toMatch(/<option(?=[^>]*value="base")(?=[^>]*selected="")[^>]*>表示名 base/);
    expect(html).toMatch(/<option(?=[^>]*value="saver")(?=[^>]*disabled="")[^>]*>表示名 saver/);
    expect(html).toMatch(/<option(?=[^>]*value="medium")(?=[^>]*selected="")[^>]*>medium/);
    expect(html).toContain("変えない");
    expect(html).not.toContain('value="high"');
  });

  it("preserves unlisted selections, falls back to input, and keeps models out of general settings", () => {
    for (const list of [null, []]) {
      const html = renderToStaticMarkup(<CombinationPicker label="基本" value={{ model: "unknown", effort: null }} models={list} onChange={() => {}} />);
      expect(html).toContain('value="unknown"');
      expect(html).toContain("モデル名を入力してください");
      expect(html).not.toContain("一覧に無いモデル");
    }
    const html = renderToStaticMarkup(<CombinationPicker label="基本" value={{ model: "unknown", effort: "high" }} models={models} onChange={() => {}} />);
    expect(html).toContain("unknown（一覧に無いモデル）");
    expect(html).toMatch(/<select(?=[^>]*aria-label="基本の推論の強さ")(?=[^>]*disabled="")[^>]*><option value="" selected="">変えない<\/option><\/select>/);
    expect(html).toContain("このモデルは推論の強さを選べません");
    expect(html).not.toContain('value="high"');
    const general = renderToStaticMarkup(<SettingsDialog settings={settings} onSave={noop} onClose={() => {}} />);
    expect(general).not.toContain("config.toml");
    expect(general).not.toContain("モデル切り替え");
  });

  it("confirms the exact manual combination and offers cancel before committing", () => {
    const target = { model: "saver", effort: "medium" };
    const html = renderToStaticMarkup(<ManualSwitchConfirmation name="Codex" target={target} busy={false} onCancel={() => {}} onConfirm={() => {}} />);
    expect(html).toContain(switchConfirmation("Codex", target));
    expect(html).toContain("キャンセル");
    expect(html).toContain("config.toml");
    expect(switchConfirmation("Codex", { ...target, effort: null })).toContain("推論 変えない");
  });

  it("keeps realistic listed models, current settings and latest history in collapsed summaries", () => {
    const realisticModels: ModelInfo[] = [
      { ...models[0], slug: "gpt-6.1-sol", display_name: "GPT-6.1-Sol", supported_reasoning_levels: [{ effort: "high", description: "詳しく考える" }] },
      { ...models[1], slug: "gpt-6-luna", display_name: "GPT-6-Luna" },
      { ...models[0], slug: "plain", display_name: "推論なし", supported_reasoning_levels: [] },
    ];
    const realisticSettings: Settings = { ...settings, agents: { codex: { ...settings.agents.codex,
      base: { model: "gpt-6.1-sol", effort: "high" }, saver: { model: "gpt-6-luna", effort: null } } } };
    const realisticStatus: ModelSwitchStatus = { ...status, models: realisticModels, currentModel: "gpt-6.1-sol", currentEffort: "high",
      usage: { fiveHour: status.usage!.fiveHour, week: { usedPercent: 3, resetsAt: new Date(2026, 9, 22, 9).getTime() / 1000 } },
      history: [{ ...status.history[0], trigger: "manual", before: { model: "gpt-6.1-sol", effort: "high" }, after: { model: "gpt-6-luna", effort: null } }] };
    const html = renderToStaticMarkup(<ModelsPage settings={realisticSettings} status={realisticStatus} now={now} onSave={noop} onManual={noop} />);
    expect(html).not.toContain("一覧に無いモデル");
    expect(html.match(/aria-expanded="false"/g)).toHaveLength(3);
    expect(html.match(/class="model-panel" hidden=""/g)).toHaveLength(3);
    expect(html).toContain("今: GPT-6.1-Sol · high</span>");
    expect(html).toContain("オン · 90%以上で GPT-6.1-Sol(high) → GPT-6-Luna(変えない)</span>");
    expect(html).toContain("最新: 10/18 10:00 手動で GPT-6-Luna へ（全1件）</span>");
    expect(html).toContain("10/22 09:00 にリセット");
    const withoutEffort = renderToStaticMarkup(<ModelsPage settings={realisticSettings}
      status={{ ...realisticStatus, currentModel: "plain", currentEffort: "high" }} now={now} onSave={noop} onManual={noop} />);
    expect(withoutEffort).not.toContain("一覧に無いモデル");
    expect(withoutEffort).toMatch(/<select(?=[^>]*aria-label="今すぐ使う組み合わせの推論の強さ")(?=[^>]*disabled="")[^>]*><option value="" selected="">変えない<\/option><\/select>/);
    expect(withoutEffort).toContain("このモデルは推論の強さを選べません");
  });

  it("connects an expanded header to its panel and hides collapsed panels", () => {
    for (const open of [false, true]) {
      const html = renderToStaticMarkup(<ModelSection title="手動の切り替え" summary="今: GPT-6.1-Sol · high" open={open} onToggle={() => {}}>操作</ModelSection>);
      const controls = html.match(/aria-controls="([^"]+)"/)![1];
      expect(html).toContain(`aria-expanded="${open}"`);
      expect(html).toContain(`id="${controls}" role="region"`);
      expect(html.includes('hidden=""')).toBe(!open);
    }
  });

  it("orders history by time and summarizes the newest switch, including off and empty states", () => {
    const html = renderToStaticMarkup(<ModelsPage settings={{ ...settings, agents: { codex: { ...settings.agents.codex, autoSwitch: false } } }}
      status={{ ...status, history: [status.history[0], { ...status.history[0], time: now / 1000 + 60, trigger: "manual", after: { model: "base", effort: null } }] }}
      now={now} onSave={noop} onManual={noop} />);
    expect(html).toContain("オフ · 90%以上で 表示名 base(変えない) → 表示名 saver(low)</span>");
    expect(html).toContain("最新: 10/18 10:01 手動で 表示名 base へ（全2件）</span>");
    const entries = html.slice(html.indexOf('<ol class="model-history"'));
    expect(entries.indexOf("10:01")).toBeLessThan(entries.indexOf("10:00"));
    const empty = renderToStaticMarkup(<ModelsPage settings={settings} status={{ ...status, history: [], usage: null }} now={now} onSave={noop} onManual={noop} />);
    expect(empty).toContain('title="記録なし">記録なし</span>');
    expect(empty).not.toContain('<ol class="model-history"');
  });
});

describe("model choices and navigation", () => {
  it("validates unavailable models and unsupported efforts while allowing fallback input", () => {
    expect(modelName("saver", models)).toBe("表示名 saver");
    expect(modelName("unknown", models)).toBe("unknown（一覧に無いモデル）");
    expect(modelName("unknown", null)).toBe("unknown");
    expect(combinationError({ model: "unknown", effort: null }, null)).toBeNull();
    expect(combinationError({ model: "unknown", effort: null }, models)).toContain("選び直し");
    expect(combinationError({ model: "base", effort: "high" }, models)).toContain("推論");
    expect(combinationError({ model: "base", effort: "medium" }, models)).toBeNull();
    expect(combinationError({ model: "bad\nmodel", effort: null }, null)).toBeTruthy();
  });

  it("clears effort for absent, unsupported and unavailable models and preserves supported levels", () => {
    const value = { model: "base", effort: "medium" };
    expect(normalizeCombination(value, models)).toEqual(value);
    expect(normalizeCombination({ ...value, effort: "high" }, models).effort).toBeNull();
    expect(normalizeCombination(value, [{ ...models[0], supported_reasoning_levels: [] }]).effort).toBeNull();
    expect(normalizeCombination({ ...value, model: "unknown" }, models).effort).toBeNull();
    expect(normalizeCombination(value, null).effort).toBeNull();
    expect(normalizeCombination(value, []).effort).toBeNull();
  });

  it("labels the existing all view ログ in the top navigation", () => {
    const html = renderToStaticMarkup(<App />);
    expect(html).toContain("実行中 0</button>");
    expect(html).toContain(">ログ</button>");
    expect(html).toContain(">モデル</button>");
    expect(html).not.toContain(">すべて</button>");
  });

  it("formats today and other days in local time, including a year boundary", () => {
    expect(resetTime(new Date(2026, 9, 18, 14, 30).getTime() / 1000, now)).toBe("14:30 にリセット");
    expect(resetTime(new Date(2026, 9, 22, 9).getTime() / 1000, now)).toBe("10/22 09:00 にリセット");
    expect(resetTime(new Date(2027, 9, 18, 9).getTime() / 1000, now)).toBe("10/18 09:00 にリセット");
  });

  it("opens the models view without losing selection and returns to all for a focused job", () => {
    const state = reducer({ ...initialState, selected: "job" }, { type: "view", view: "models" });
    expect(state.view).toBe("models");
    expect(state.selected).toBe("job");
    expect(reducer(state, { type: "openJob", id: "focused" })).toMatchObject({ view: "all", selected: "focused" });
  });
});
