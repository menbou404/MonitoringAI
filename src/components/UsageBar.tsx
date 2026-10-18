import type { ModelSwitchStatus } from "../bindings/ModelSwitchStatus";
import { usagePercent } from "../model/format";

export function UsageBar({ status, threshold }: { status: ModelSwitchStatus; threshold: number }) {
  return <div className="usage-bar" aria-label="Codexの使用率" aria-live="polite">
    <span className={status.usage?.fiveHour && status.usage.fiveHour.usedPercent >= threshold ? "usage-high" : ""}>5時間 {usagePercent(status.usage?.fiveHour)}</span>
    <span aria-hidden="true">・</span>
    <span className={status.usage?.week && status.usage.week.usedPercent >= threshold ? "usage-high" : ""}>週 {usagePercent(status.usage?.week)}</span>
    <span className="usage-model">既定: {status.currentModel ?? "不明"}</span>
    {status.switchedByApp && <span className="usage-switched">節約用に切り替え中</span>}
    {status.error && <span className="field-error" role="alert">{status.error}</span>}
  </div>;
}
