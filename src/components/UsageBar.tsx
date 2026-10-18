import type { ModelSwitchStatus } from "../bindings/ModelSwitchStatus";
import { modelName } from "../model/models";
import { usagePercent } from "../model/format";

export function UsageBar({ status, threshold, onOpen }: { status: ModelSwitchStatus; threshold: number; onOpen: () => void }) {
  return <button type="button" onClick={onOpen} className="usage-bar" aria-label="モデルページを開く：Codexの使用率" aria-live="polite">
    <span className={status.usage?.fiveHour && status.usage.fiveHour.usedPercent >= threshold ? "usage-high" : ""}>5時間 {usagePercent(status.usage?.fiveHour)}</span>
    <span aria-hidden="true">・</span>
    <span className={status.usage?.week && status.usage.week.usedPercent >= threshold ? "usage-high" : ""}>週 {usagePercent(status.usage?.week)}</span>
    <span className="usage-model">既定: {modelName(status.currentModel, status.models)}</span>
    {status.switchedByApp && <span className="usage-switched">節約用に切り替え中</span>}
    {status.error && <span className="field-error" role="alert">{status.error}</span>}
  </button>;
}
