import { useEffect, useId, useRef, useState } from "react";
import type { AgentModelSettings } from "../bindings/AgentModelSettings";
import type { ModelAgent } from "../bindings/ModelAgent";
import type { ModelCombination } from "../bindings/ModelCombination";
import type { ModelInfo } from "../bindings/ModelInfo";
import type { ModelSwitchStatus } from "../bindings/ModelSwitchStatus";
import type { Settings } from "../bindings/Settings";
import type { UsageWindow } from "../bindings/UsageWindow";
import { usagePercent } from "../model/format";
import { combinationError, modelName, resetTime, switchConfirmation } from "../model/models";

export function CombinationPicker({ label, value, models, excluded, disabled, onChange }: {
  label: string; value: ModelCombination; models: ModelInfo[] | null; excluded?: string; disabled?: boolean;
  onChange: (value: ModelCombination) => void;
}) {
  const id = useId();
  const listed = models?.length ? models : null;
  const selected = listed?.find((model) => model.slug === value.model);
  const levels = selected?.supported_reasoning_levels ?? [];
  const unsupported = value.effort !== null && !levels.some((level) => level.effort === value.effort);
  return <fieldset className="combination-picker" disabled={disabled}><legend>{label}</legend>
    {listed ? <>
      {!selected && <p className="field-error">{value.model || "未選択"}（一覧に無いモデル）。選び直してください。</p>}
      <div className="model-options" role="radiogroup" aria-label={`${label}のモデル`}>
        {listed.map((model) => <label className="model-option" key={model.slug}>
          <input type="radio" name={id} value={model.slug} checked={value.model === model.slug} disabled={model.slug === excluded}
            onChange={() => onChange({ model: model.slug, effort: value.effort !== null && model.supported_reasoning_levels.some((level) => level.effort === value.effort) ? value.effort : null })} />
          <span><strong>{model.display_name}</strong><small>{model.description}</small></span>
        </label>)}
      </div>
    </> : <label className="field">モデル名<input value={value.model} onChange={(event) => onChange({ ...value, model: event.target.value })} />
      <small className="settings-note">モデルの一覧を取得できません。モデル名を入力してください。</small></label>}
    <label className="field">推論の強さ<select value={value.effort ?? ""} onChange={(event) => onChange({ ...value, effort: event.target.value || null })}>
      <option value="">変えない</option>
      {unsupported && <option value={value.effort!} disabled>{value.effort}（対応を確認できません）</option>}
      {levels.map((level) => <option value={level.effort} key={level.effort}>{level.effort}{level.description ? ` — ${level.description}` : ""}</option>)}
    </select></label>
  </fieldset>;
}

function UsageMeter({ label, window, threshold, now }: { label: string; window: UsageWindow | null | undefined; threshold: number; now: number }) {
  const high = window && window.usedPercent >= threshold;
  return <div className={`usage-meter${high ? " usage-high" : ""}`}>
    <div><strong>{label}</strong><span>{window ? usagePercent(window) : "まだ記録がありません"}</span>
      {window && <small>{resetTime(window.resetsAt, now)}</small>}</div>
    {window && <progress aria-label={`${label}の使用率`} value={Math.min(100, window.usedPercent)} max={100} />}
  </div>;
}

type AgentSectionProps = { agent: ModelAgent; name: string; settings: AgentModelSettings; status: ModelSwitchStatus; now: number;
  onSave: (agent: ModelAgent, settings: AgentModelSettings) => Promise<void>;
  onManual: (agent: ModelAgent, target: ModelCombination) => Promise<void> };

export function ManualSwitchConfirmation({ name, target, busy, onCancel, onConfirm }: {
  name: string; target: ModelCombination; busy: boolean; onCancel: () => void; onConfirm: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  return <dialog ref={dialog} className="settings-dialog model-confirmation" aria-label="既定の変更を確認"
    onCancel={(event) => { event.preventDefault(); if (!busy) onCancel(); }}>
    <header><h2>既定の変更を確認</h2></header><div className="settings-body"><p>{switchConfirmation(name, target)}</p>
      <p className="settings-note">config.toml を書き換えます。切り替え後に始まるスレッドから反映されます。</p></div>
    <footer><button disabled={busy} autoFocus onClick={onCancel}>キャンセル</button><button disabled={busy} className="primary" onClick={onConfirm}>既定にする</button></footer>
  </dialog>;
}

export function AgentModelsSection({ agent, name, settings, status, now, onSave, onManual }: AgentSectionProps) {
  const [draft, setDraft] = useState(settings);
  const [dirty, setDirty] = useState(false);
  const [manual, setManual] = useState<ModelCombination>({ model: status.currentModel ?? settings.base.model, effort: null });
  const [confirm, setConfirm] = useState<ModelCombination | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  useEffect(() => { if (!dirty) setDraft(settings); }, [settings, dirty]);
  const change = (value: AgentModelSettings) => { setDraft(value); setDirty(true); setMessage(null); };
  const automaticError = !Number.isInteger(draft.threshold) || draft.threshold < 1 || draft.threshold > 100 ? "しきい値は1〜100の整数にしてください"
    : draft.base.model === draft.saver.model ? "基本と節約用には異なるモデルを選んでください"
    : combinationError(draft.base, status.models) ?? combinationError(draft.saver, status.models);
  const manualError = combinationError(manual, status.models);
  async function save() {
    if (automaticError || busy) return;
    setBusy(true); setError(null); setMessage(null);
    try { await onSave(agent, draft); setDirty(false); setMessage("設定を保存しました"); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  async function apply() {
    if (!confirm || busy) return;
    const target = confirm;
    setBusy(true); setError(null); setMessage(null);
    try { await onManual(agent, target); setConfirm(null); setMessage("既定を変更しました"); }
    catch (e) { setConfirm(null); setError(String(e)); }
    finally { setBusy(false); }
  }
  return <section className="agent-models" aria-label={`${name}のモデル`}>
    <h1>{name}</h1>
    <section className="model-section"><h2>使用率</h2>
      <UsageMeter label="5時間枠" window={status.usage?.fiveHour} threshold={settings.threshold} now={now} />
      <UsageMeter label="週枠" window={status.usage?.week} threshold={settings.threshold} now={now} />
    </section>
    <section className="model-section"><h2>現在の既定</h2>
      <p>{modelName(status.currentModel, status.models)}{status.currentEffort ? ` · 推論 ${status.currentEffort}` : " · 推論 未設定"}</p>
      {status.switchedByApp && <p className="usage-switched">自動で節約用に切り替え中</p>}
      {status.error && <p className="field-error" role="alert">{status.error}</p>}
    </section>
    <section className="model-section"><h2>手動の切り替え</h2>
      <CombinationPicker label="今すぐ使う組み合わせ" value={manual} models={status.models} disabled={busy} onChange={setManual} />
      {manualError && <p className="field-error">{manualError}</p>}
      <button disabled={busy || !!manualError} onClick={() => setConfirm({ ...manual })}>今すぐ既定にする</button>
    </section>
    <section className="model-section"><h2>自動の切り替え</h2>
      <label className="switch"><input type="checkbox" checked={draft.autoSwitch} disabled={busy} onChange={(event) => change({ ...draft, autoSwitch: event.target.checked })} />使用率に応じて切り替える</label>
      <p className="settings-note">オンにすると、Codexの config.toml にある既定のモデルと推論の強さを書き換えます。切り替え後に始まるスレッドから反映されます。オフにしても現在のモデルは戻しません。</p>
      <label className="field">切り替えるしきい値（%）<input type="number" min={1} max={100} step={1} value={draft.threshold} disabled={busy}
        onChange={(event) => change({ ...draft, threshold: Number(event.target.value) })} /></label>
      <div className="automatic-combinations">
        <CombinationPicker label="基本の組み合わせ" value={draft.base} models={status.models} excluded={draft.saver.model} disabled={busy} onChange={(base) => change({ ...draft, base })} />
        <CombinationPicker label="節約用の組み合わせ" value={draft.saver} models={status.models} excluded={draft.base.model} disabled={busy} onChange={(saver) => change({ ...draft, saver })} />
      </div>
      {automaticError && <p className="field-error">{automaticError}</p>}
      <button className="primary" disabled={busy || !!automaticError} onClick={save}>保存</button>
    </section>
    {error && <p className="field-error" role="alert">{error}</p>}
    {message && <p role="status">{message}</p>}
    <section className="model-section"><h2>切り替えの履歴</h2>
      {status.history.length === 0 ? <p className="settings-note">まだ切り替えの記録がありません</p> : <ol className="model-history">
        {status.history.filter((entry) => entry.agent === agent).map((entry, index) => <li key={`${entry.time}-${index}`}>
          <div><time dateTime={new Date(entry.time * 1000).toISOString()}>{new Date(entry.time * 1000).toLocaleString("ja-JP")}</time>
            <strong>{{ "auto-saver": "自動で節約用へ", "auto-revert": "自動で戻す", manual: "手動" }[entry.trigger]}</strong></div>
          <p>{modelName(entry.before.model, status.models)} · 推論 {entry.before.effort ?? "未設定"} → {modelName(entry.after.model, status.models)} · 推論 {entry.after.effort ?? "未設定"}</p>
          <small>5時間 {usagePercent(entry.usage?.fiveHour)}・週 {usagePercent(entry.usage?.week)}</small>
        </li>)}
      </ol>}
    </section>
    {confirm && <ManualSwitchConfirmation name={name} target={confirm} busy={busy} onCancel={() => setConfirm(null)} onConfirm={apply} />}
  </section>;
}

export function ModelsPage({ settings, status, now, onSave, onManual }: {
  settings: Settings; status: ModelSwitchStatus; now: number;
  onSave: AgentSectionProps["onSave"]; onManual: AgentSectionProps["onManual"];
}) {
  const agents: Pick<AgentSectionProps, "agent" | "name" | "settings" | "status">[] = [
    { agent: "codex", name: "Codex", settings: settings.agents.codex, status },
  ];
  return <main className="models-page">{agents.map((entry) => <AgentModelsSection key={entry.agent} {...entry} now={now} onSave={onSave} onManual={onManual} />)}</main>;
}
