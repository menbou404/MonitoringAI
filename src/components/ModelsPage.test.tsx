import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { ModelInfo } from "../bindings/ModelInfo";
import type { ModelSwitchStatus } from "../bindings/ModelSwitchStatus";
import type { Settings } from "../bindings/Settings";
import { combinationError, modelName, resetTime, switchConfirmation } from "../model/models";
import { initialState, reducer } from "../model/store";
import { CombinationPicker, ManualSwitchConfirmation, ModelsPage } from "./ModelsPage";
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
    expect(html).toContain("まだ記録がありません");
    expect(html).toContain("表示名 saver · 推論 medium");
    expect(html).toContain("自動で節約用に切り替え中");
    expect(html).toContain("config.toml");
    expect(html).toContain("今すぐ既定にする");
    expect(html).toContain("自動で節約用へ");
    expect(html).toContain("推論 high → 表示名 saver · 推論 medium");
    expect(html).not.toContain("既定の変更を確認");
  });

  it("offers descriptions, only supported efforts and do-not-touch, and excludes the other model", () => {
    const html = renderToStaticMarkup(<CombinationPicker label="基本" value={{ model: "base", effort: "medium" }} models={models} excluded="saver" onChange={() => {}} />);
    expect(html).toContain("説明 base");
    expect(html).toContain('name="');
    expect(html).toMatch(/<input(?=[^>]*value="base")(?=[^>]*checked="")[^>]*>/);
    expect(html).toMatch(/disabled=""[^>]*value="saver"/);
    expect(html).toContain('value="medium" selected=""');
    expect(html).toContain("変えない");
    expect(html).not.toContain('value="high"');
  });

  it("preserves unlisted selections, falls back to input, and keeps models out of general settings", () => {
    for (const list of [null, []]) {
      const html = renderToStaticMarkup(<CombinationPicker label="基本" value={{ model: "unknown", effort: null }} models={list} onChange={() => {}} />);
      expect(html).toContain('value="unknown"');
      expect(html).toContain("モデル名を入力してください");
    }
    const html = renderToStaticMarkup(<CombinationPicker label="基本" value={{ model: "unknown", effort: "high" }} models={models} onChange={() => {}} />);
    expect(html).toContain("unknown（一覧に無いモデル）");
    expect(html).toContain("対応を確認できません");
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
});

describe("model choices and navigation", () => {
  it("validates unavailable models and unsupported efforts while allowing fallback input", () => {
    expect(modelName("saver", models)).toBe("表示名 saver");
    expect(modelName("unknown", models)).toBe("unknown（一覧に無いモデル）");
    expect(combinationError({ model: "unknown", effort: null }, null)).toBeNull();
    expect(combinationError({ model: "unknown", effort: null }, models)).toContain("選び直し");
    expect(combinationError({ model: "base", effort: "high" }, models)).toContain("推論");
    expect(combinationError({ model: "base", effort: "medium" }, models)).toBeNull();
    expect(combinationError({ model: "bad\nmodel", effort: null }, null)).toBeTruthy();
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
