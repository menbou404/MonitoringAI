import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import type { AgentModelSettings } from "../bindings/AgentModelSettings";
import type { ModelAgent } from "../bindings/ModelAgent";
import type { ModelCombination } from "../bindings/ModelCombination";
import type { ModelInfo } from "../bindings/ModelInfo";
import type { ModelSwitchStatus } from "../bindings/ModelSwitchStatus";
import type { Settings } from "../bindings/Settings";
import type { UsageWindow } from "../bindings/UsageWindow";
import { usagePercent } from "../model/format";
import { combinationError, modelName, normalizeCombination, resetTime, switchConfirmation } from "../model/models";

export function CombinationPicker({ label, value, models, excluded, disabled, onChange }: {
  label: string; value: ModelCombination; models: ModelInfo[] | null; excluded?: string; disabled?: boolean;
  onChange: (value: ModelCombination) => void;
}) {
  const listed = models?.length ? models : null;
  const selected = listed?.find((model) => model.slug === value.model);
  const levels = selected?.supported_reasoning_levels ?? [];
  const normalized = normalizeCombination(value, models);
  return <fieldset className="combination-picker" disabled={disabled}><legend>{label}</legend>
    {listed ? <label className="field model-choice"><select aria-label={`${label}のモデル`} value={value.model}
      onChange={(event) => onChange(normalizeCombination({ ...value, model: event.target.value }, models))}>
      {!selected && <option value={value.model} disabled>{value.model || "未選択"}（一覧に無いモデル）</option>}
      {listed.map((model) => <option key={model.slug} value={model.slug} disabled={model.slug === excluded}>{model.display_name}</option>)}
    </select><small className={selected ? "model-description" : "model-description field-error"} title={selected?.description}>
      {selected ? selected.description : "一覧から選び直してください"}</small></label>
      : <label className="field model-choice"><input aria-label={`${label}のモデル名`} value={value.model}
        onChange={(event) => onChange({ model: event.target.value, effort: null })} />
      <small className="model-description">一覧なし。モデル名を入力してください。</small></label>}
    <label className="effort-choice">推論の強さ<select aria-label={`${label}の推論の強さ`} disabled={levels.length === 0} value={normalized.effort ?? ""}
      onChange={(event) => onChange({ ...value, effort: event.target.value || null })}>
      <option value="">変えない</option>
      {levels.map((level) => <option value={level.effort} key={level.effort} title={level.description}>{level.effort}</option>)}
    </select></label>
    {levels.length === 0 && <small className="picker-note">このモデルは推論の強さを選べません</small>}
  </fieldset>;
}

function UsageMeter({ label, window, threshold, now }: { label: string; window: UsageWindow | null | undefined; threshold: number; now: number }) {
  const high = window && window.usedPercent >= threshold;
  return <div className={`usage-meter${high ? " usage-high" : ""}`}>
    <div><strong>{label}</strong><span>{window ? usagePercent(window) : "記録なし"}</span></div>
    {window && <progress aria-label={`${label}の使用率`} value={Math.min(100, window.usedPercent)} max={100} />}
    {window && <small>{resetTime(window.resetsAt, now)}</small>}
  </div>;
}

export function ModelSection({ title, summary, open, onToggle, children }: {
  title: string; summary: string; open: boolean; onToggle: () => void; children: ReactNode;
}) {
  const id = useId();
  return <section className={`model-section${open ? " open" : ""}`}>
    <h2><button type="button" id={`${id}-heading`} className="model-section-toggle" aria-expanded={open} aria-controls={id} onClick={onToggle}>
      <span aria-hidden="true">{open ? "▾" : "▸"}</span><strong>{title}</strong><span className="model-section-summary" title={summary}>{summary}</span>
    </button></h2>
    <div id={id} role="region" aria-labelledby={`${id}-heading`} className="model-panel" hidden={!open}>{children}</div>
  </section>;
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
  const [open, setOpen] = useState<"manual" | "automatic" | "history" | null>(null);
  useEffect(() => { if (!dirty) setDraft(settings); }, [settings, dirty]);
  const change = (value: AgentModelSettings) => { setDraft(value); setDirty(true); setMessage(null); };
  const base = normalizeCombination(draft.base, status.models);
  const saver = normalizeCombination(draft.saver, status.models);
  const manualTarget = normalizeCombination(manual, status.models);
  const automaticError = !Number.isInteger(draft.threshold) || draft.threshold < 1 || draft.threshold > 100 ? "しきい値は1〜100の整数にしてください"
    : draft.base.model === draft.saver.model ? "基本と節約用には異なるモデルを選んでください"
    : combinationError(base, status.models) ?? combinationError(saver, status.models);
  const manualError = combinationError(manualTarget, status.models);
  const current = `${modelName(status.currentModel, status.models)} · ${status.currentEffort ?? "未設定"}`;
  const combinationSummary = (value: ModelCombination) => `${modelName(value.model, status.models)}(${normalizeCombination(value, status.models).effort ?? "変えない"})`;
  const history = status.history.filter((entry) => entry.agent === agent).sort((a, b) => b.time - a.time).slice(0, 100);
  const latest = history[0];
  const historyTime = (seconds: number) => {
    const date = new Date(seconds * 1000);
    return `${date.getMonth() + 1}/${date.getDate()} ${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
  };
  const trigger = { "auto-saver": "自動で節約用へ", "auto-revert": "自動で戻す", manual: "手動" };
  async function save() {
    if (automaticError || busy) return;
    setBusy(true); setError(null); setMessage(null);
    try { await onSave(agent, { ...draft, base, saver }); setDirty(false); setMessage("設定を保存しました"); }
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
    <header className="model-summary"><h1>{name}</h1>
      <UsageMeter label="5時間" window={status.usage?.fiveHour} threshold={settings.threshold} now={now} />
      <UsageMeter label="週" window={status.usage?.week} threshold={settings.threshold} now={now} />
      <div className="model-current"><div>現在の既定 {status.switchedByApp && <span className="usage-switched">節約中</span>}</div><p title={current}>{current}</p></div>
    </header>
    {status.error && <p className="model-feedback field-error" role="alert" title={status.error}>{status.error}</p>}
    {error && <p className="model-feedback field-error" role="alert" title={error}>{error}</p>}
    {message && <p className="model-feedback" role="status">{message}</p>}
    <ModelSection title="手動の切り替え" summary={`今: ${current}`} open={open === "manual"} onToggle={() => setOpen(open === "manual" ? null : "manual")}>
      <CombinationPicker label="今すぐ使う組み合わせ" value={manualTarget} models={status.models} disabled={busy} onChange={setManual} />
      <div className="model-actions"><button disabled={busy || !!manualError} onClick={() => setConfirm({ ...manualTarget })}>今すぐ既定にする</button>
        {manualError && <span className="field-error" title={manualError}>{manualError}</span>}</div>
    </ModelSection>
    <ModelSection title="自動の切り替え"
      summary={`${settings.autoSwitch ? "オン" : "オフ"} · ${settings.threshold}%以上で ${combinationSummary(settings.base)} → ${combinationSummary(settings.saver)}`}
      open={open === "automatic"} onToggle={() => setOpen(open === "automatic" ? null : "automatic")}>
      <div className="automatic-controls">
        <label className="switch"><input type="checkbox" checked={draft.autoSwitch} disabled={busy} onChange={(event) => change({ ...draft, autoSwitch: event.target.checked })} />オン</label>
        <button type="button" className="model-help" aria-label="自動の切り替えについて"
          title="オンにすると、Codexの config.toml の既定モデルと推論の強さを書き換えます。新しいスレッドから反映されます。オフにしても現在のモデルは戻しません。">？</button>
        {automaticError && <span className="field-error" title={automaticError}>{automaticError}</span>}
        <label className="threshold-choice">しきい値 <input type="number" min={1} max={100} step={1} value={draft.threshold} disabled={busy}
          onChange={(event) => change({ ...draft, threshold: Number(event.target.value) })} /> %</label>
        <button className="primary" disabled={busy || !!automaticError} onClick={save}>保存</button>
      </div>
      <div className="automatic-combinations">
        <CombinationPicker label="基本" value={base} models={status.models} excluded={saver.model} disabled={busy} onChange={(base) => change({ ...draft, base })} />
        <CombinationPicker label="節約用" value={saver} models={status.models} excluded={base.model} disabled={busy} onChange={(saver) => change({ ...draft, saver })} />
      </div>
    </ModelSection>
    <ModelSection title="切り替えの履歴"
      summary={latest ? `最新: ${historyTime(latest.time)} ${latest.trigger === "manual" ? "手動で" : "自動で"} ${modelName(latest.after.model, status.models)} へ（全${history.length}件）` : "記録なし"}
      open={open === "history"} onToggle={() => setOpen(open === "history" ? null : "history")}>
      {history.length === 0 ? <p className="settings-note">記録なし</p> : <ol className="model-history" tabIndex={0} aria-label="切り替えの履歴一覧">
        {history.map((entry, index) => <li key={`${entry.time}-${index}`}>
          <div><time dateTime={new Date(entry.time * 1000).toISOString()}>{historyTime(entry.time)}</time><strong>{trigger[entry.trigger]}</strong></div>
          <p>{modelName(entry.before.model, status.models)} · {entry.before.effort ?? "未設定"} → {modelName(entry.after.model, status.models)} · {entry.after.effort ?? "未設定"}</p>
          <small>5時間 {usagePercent(entry.usage?.fiveHour)}・週 {usagePercent(entry.usage?.week)}</small>
        </li>)}
      </ol>}
    </ModelSection>
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
