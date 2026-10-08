import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { JobSummary } from "../bindings/JobSummary";
import type { Health } from "../bindings/Health";
import { clock, elapsedJob, requestLead, stateName, workspaceName } from "../model/format";

type Row = { kind: "heading"; label: string } | { kind: "job"; job: JobSummary; active: boolean };
export function JobList({ active, recent, selected, unseen, health, now, onSelect }: {
  active: JobSummary[]; recent: JobSummary[]; selected: string | null; unseen: string[];
  health: Health; now: number; onSelect: (id: string) => void;
}) {
  const parent = useRef<HTMLDivElement>(null);
  const rows: Row[] = [{ kind: "heading", label: `実行中 (${active.length})` }, ...active.map((job) => ({ kind: "job" as const, job, active: true })),
    { kind: "heading", label: "最近" }, ...recent.map((job) => ({ kind: "job" as const, job, active: false }))];
  const virtual = useVirtualizer({ count: rows.length, getScrollElement: () => parent.current,
    estimateSize: (index) => rows[index].kind === "heading" ? 38 : 90, overscan: 6 });
  useEffect(() => {
    const index = rows.findIndex((row) => row.kind === "job" && row.job.id === selected);
    if (index >= 0) virtual.scrollToIndex(index, { align: "auto" });
  }, [selected]);
  return <div className="job-scroll" ref={parent} role="listbox" aria-label="ジョブ一覧">
    {!active.length && !recent.length && <div className="empty-jobs">Codexのジョブはまだありません
      <small>監視中の状態ルート{health.watchedRoots.length ? health.watchedRoots.map((root) => <span key={root}>{root}</span>) : <span>なし</span>}</small>
    </div>}
    <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
      {virtual.getVirtualItems().map((item) => {
        const row = rows[item.index];
        return <div key={item.key} ref={virtual.measureElement} data-index={item.index}
          style={{ position: "absolute", top: item.start, width: "100%" }}>
          {row.kind === "heading" ? <div className="group-heading">{row.label}</div> :
            <button type="button" role="option" aria-selected={selected === row.job.id}
              className={`job-row ${selected === row.job.id ? "selected" : ""}`} onClick={() => onSelect(row.job.id)}>
              <span className="job-primary"><span className={`state-icon ${row.job.effectiveState}`} aria-hidden="true">{icon(row.job)}</span>
                {unseen.includes(row.job.id) && <span className="unseen-dot" aria-label="未確認" />}
                <strong>{workspaceName(row.job)}</strong>
                {row.job.kindLabel && row.job.kindLabel !== "rescue" && <em>{row.job.kindLabel}</em>}
                <time>{row.active ? elapsedJob(row.job, now) : clock(row.job.completedAt ?? row.job.updatedAt)}</time></span>
              <span className="job-request">{requestLead(row.job)}</span>
              <span className="job-secondary">{row.active ? `${stateName(row.job.effectiveState, row.job.phase, row.job.status)}${row.job.latestMessage ? ` · ${row.job.latestMessage}` : ""}` : stateName(row.job.effectiveState, row.job.phase, row.job.status)}</span>
            </button>}
        </div>;
      })}
    </div>
  </div>;
}
function icon(job: JobSummary) {
  switch (job.effectiveState) {
    case "queued": return "○"; case "running": case "quiet": return "●";
    case "orphaned": return "!"; case "completed": return "✓";
    case "failed": return "✕"; case "cancelled": return "－"; default: return "?";
  }
}
