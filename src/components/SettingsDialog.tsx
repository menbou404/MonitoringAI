import { useState } from "react";
import type { Settings } from "../bindings/Settings";

const notificationKeys = [
  ["started", "開始"], ["completed", "完了"], ["failed", "失敗"],
  ["orphaned", "停止した可能性"], ["quiet", "応答なし"],
] as const;
function numericError(value: number, min: number, max: number) {
  return Number.isInteger(value) && value >= min && value <= max ? null : `${min}〜${max}の整数を入力してください`;
}
export function SettingsDialog({ settings, onClose, onSave }: { settings: Settings; onClose: () => void; onSave: (value: Settings) => Promise<void> }) {
  const [draft, setDraft] = useState(settings);
  const [roots, setRoots] = useState(settings.extra_state_roots.join("\n"));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rootValues = roots.split(/\r?\n/).map((s) => s.trim()).filter(Boolean);
  const errors = {
    quiet_minutes: numericError(draft.quiet_minutes, 1, 120),
    list_days: numericError(draft.list_days, 1, 30),
    history_days: numericError(draft.history_days, 1, 90) ?? (draft.history_days < draft.list_days ? "一覧の期間以上にしてください" : null),
    roots: rootValues.every((s) => /^[A-Za-z]:[\\/]/.test(s) || /^\\\\[^\\]+\\[^\\]+/.test(s)) ? null : "絶対パスを1行ずつ入力してください",
  };
  const invalid = Object.values(errors).some(Boolean);
  async function save() {
    if (invalid) return;
    setBusy(true); setError(null);
    try { await onSave({ ...draft, extra_state_roots: rootValues }); onClose(); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  }
  const numberField = (label: string, key: "quiet_minutes" | "list_days" | "history_days") =>
    <label className="field">{label}<input type="number" value={draft[key]} onChange={(e) => setDraft({ ...draft, [key]: Number(e.target.value) })} />
      {errors[key] && <small className="field-error">{errors[key]}</small>}</label>;
  return <div className="dialog-backdrop" onMouseDown={onClose}><div className="settings-dialog" role="dialog" aria-modal="true" aria-label="設定" onMouseDown={(e) => e.stopPropagation()}>
    <header><h2>設定</h2><button onClick={onClose} aria-label="閉じる">×</button></header>
    <div className="settings-body">
      <label className="switch"><input type="checkbox" checked={draft.notifications.enabled} onChange={(e) => setDraft({ ...draft, notifications: { ...draft.notifications, enabled: e.target.checked } })} />通知</label>
      <fieldset><legend>通知のきっかけ</legend>{notificationKeys.map(([key, label]) => <label className="switch" key={key}><input type="checkbox" checked={draft.notifications[key]}
        onChange={(e) => setDraft({ ...draft, notifications: { ...draft.notifications, [key]: e.target.checked } })} />{label}</label>)}</fieldset>
      {numberField("応答なしと判定するまでの分数", "quiet_minutes")}
      {numberField("一覧に出す終わったジョブの期間（日）", "list_days")}
      {numberField("履歴の保存期間（日）", "history_days")}
      <label className="field">追加で監視する状態ルート<textarea value={roots} onChange={(e) => setRoots(e.target.value)} rows={4} placeholder="C:\\path\\to\\state" />
        {errors.roots && <small className="field-error">{errors.roots}</small>}</label>
      <label className="switch"><input type="checkbox" checked={draft.autostart} onChange={(e) => setDraft({ ...draft, autostart: e.target.checked })} />Windows起動時に起動</label>
      {error && <div className="field-error" role="alert">{error}</div>}
    </div>
    <footer><button onClick={onClose}>キャンセル</button><button className="primary" disabled={invalid || busy} onClick={save}>保存</button></footer>
  </div></div>;
}
