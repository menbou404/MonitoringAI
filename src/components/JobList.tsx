import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { JobSummary } from "../bindings/JobSummary";
import type { Health } from "../bindings/Health";
import type { Conversation } from "../model/jobList";
import { isActive } from "../model/jobList";
import { elapsedJob, finishedTime, requestLead, stateName, workspaceName } from "../model/format";

type Row = { kind: "heading"; conversation: Conversation; open: boolean }
  | { kind: "job" | "previous"; job: JobSummary }
  | { kind: "more"; id: string; count: number };

export function JobList({ conversations, expanded, expandedChains, selected, unseen, health, now, onSelect, onToggle, onToggleChain }: {
  conversations: Conversation[]; expanded: Record<string, boolean>; expandedChains: Record<string, boolean>;
  selected: string | null; unseen: string[]; health: Health; now: number;
  onSelect: (id: string) => void; onToggle: (key: string, open: boolean) => void; onToggleChain: (id: string) => void;
}) {
  const parent = useRef<HTMLDivElement>(null);
  const rows: Row[] = conversations.flatMap((conversation) => {
    const open = expanded[conversation.key] ?? conversation.defaultOpen;
    if (!open) return [{ kind: "heading" as const, conversation, open }];
    return [{ kind: "heading" as const, conversation, open }, ...conversation.chains.flatMap((chain): Row[] => [
      { kind: "job", job: chain.latest },
      ...chain.previous.slice(0, expandedChains[chain.latest.id] ? undefined : 3).map((job): Row => ({ kind: "previous", job })),
      ...(chain.previous.length > 3 && !expandedChains[chain.latest.id] ? [{ kind: "more" as const, id: chain.latest.id, count: chain.previous.length - 3 }] : []),
    ])];
  });
  const virtual = useVirtualizer({ count: rows.length, getScrollElement: () => parent.current,
    estimateSize: (index) => rows[index].kind === "job" ? 91 : rows[index].kind === "heading" ? 40 : 35, overscan: 6 });
  useEffect(() => {
    const index = rows.findIndex((row) => (row.kind === "job" || row.kind === "previous") && row.job.id === selected);
    if (index >= 0) virtual.scrollToIndex(index, { align: "auto" });
  }, [selected, expanded, expandedChains]);
  return <div className="job-scroll" ref={parent} role="listbox" aria-label="ジョブ一覧">
    {!conversations.length && <div className="empty-jobs">Codexのジョブはまだありません
      <small>監視中の状態ルート{health.watchedRoots.length ? health.watchedRoots.map((root) => <span key={root}>{root}</span>) : <span>なし</span>}</small>
    </div>}
    <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
      {virtual.getVirtualItems().map((item) => {
        const row = rows[item.index];
        return <div key={item.key} ref={virtual.measureElement} data-index={item.index}
          style={{ position: "absolute", top: item.start, width: "100%" }}>
          {row.kind === "heading" ? <button type="button" className="group-heading" aria-expanded={row.open}
            onClick={() => onToggle(row.conversation.key, !row.open)}>
            <span>{row.open ? "▼" : "▶"} {row.conversation.title}</span>
            <small>{row.conversation.activeCount ? `実行中 ${row.conversation.activeCount}` : `（完了 ${row.conversation.count}）`}</small>
          </button> : row.kind === "more" ? <button type="button" className="more-jobs" onClick={() => onToggleChain(row.id)}>ほか {row.count}件</button>
            : row.kind === "previous" ? <button type="button" role="option" aria-selected={selected === row.job.id}
              className={`previous-row ${selected === row.job.id ? "selected" : ""}`} onClick={() => onSelect(row.job.id)}>
              ↳ 前回: {requestLead(row.job)} · {stateName(row.job.effectiveState, row.job.phase, row.job.status, row.job.archivedNote)}
            </button> : <button type="button" role="option" aria-selected={selected === row.job.id}
              className={`job-row ${selected === row.job.id ? "selected" : ""}`} onClick={() => onSelect(row.job.id)}>
              <span className="job-primary"><span className={`state-icon ${row.job.effectiveState}`} aria-hidden="true">{icon(row.job)}</span>
                {unseen.includes(row.job.id) && <span className="unseen-dot" aria-label="未確認" />}
                <strong>{requestLead(row.job)}</strong>
                {row.job.kindLabel && row.job.kindLabel !== "rescue" && <em>{row.job.kindLabel}</em>}
                <time>{isActive(row.job) ? elapsedJob(row.job, now) : finishedTime(row.job.completedAt ?? row.job.createdAt, now)}</time></span>
              {row.job.context?.requester?.depth && row.job.context.requester.depth >= 2 && row.job.context.requester.parentDescription &&
                <span className="job-parent">（{row.job.context.requester.parentDescription} から）</span>}
              <span className="job-request">{workspaceName(row.job)}{row.job.promptHead && row.job.promptHead.trim() !== requestLead(row.job) ? ` · ${row.job.promptHead}` : ""}</span>
              {isActive(row.job) && <span className="job-secondary">{stateName(row.job.effectiveState, row.job.phase, row.job.status, row.job.archivedNote)}{row.job.latestMessage ? ` · ${row.job.latestMessage}` : ""}</span>}
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
