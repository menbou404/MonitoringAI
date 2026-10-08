import type { JobSummary } from "../bindings/JobSummary";
import type { EffectiveState } from "../bindings/EffectiveState";

const phases: Record<string, string> = {
  queued: "待機中", starting: "開始中", investigating: "調査中",
  running: "コマンド実行中", verifying: "検証中", editing: "編集中",
  reviewing: "レビュー中", finalizing: "まとめ中", done: "完了",
  failed: "失敗", cancelled: "キャンセル",
};
export const phaseName = (phase: string | null) => phase ? phases[phase] ?? phase : "実行中";
export function stateName(state: EffectiveState, phase: string | null, status = ""): string {
  switch (state) {
    case "queued": return "待機中";
    case "running": return phaseName(phase);
    case "quiet": return "応答なし？";
    case "orphaned": return "停止した可能性";
    case "completed": return "完了";
    case "failed": return "失敗";
    case "cancelled": return "キャンセル";
    case "unknown": return status || "不明";
  }
}
export function elapsed(seconds: number): string {
  if (seconds < 60) return "1分未満";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}分`;
  return `${Math.floor(seconds / 3600)}時間${Math.floor(seconds % 3600 / 60)}分`;
}
export const dateMs = (value: string | null) => value ? Date.parse(value) || 0 : 0;
export const startMs = (job: JobSummary) => dateMs(job.startedAt ?? job.createdAt);
export const endMs = (job: JobSummary) => dateMs(job.completedAt ?? job.updatedAt ?? job.createdAt);
export const elapsedJob = (job: JobSummary, now: number) => elapsed(Math.max(0, (now - startMs(job)) / 1000));
export const clock = (value: string | null) => value ? new Date(value).toLocaleTimeString("ja-JP", { hour: "2-digit", minute: "2-digit" }) : "—";
export const workspaceName = (job: JobSummary) => job.workspaceName || "不明なワークスペース";
export const requestLead = (job: JobSummary) => job.summary?.trim() || job.title || "依頼内容なし";
