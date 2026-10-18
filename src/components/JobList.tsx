import { useEffect, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { JobSummary } from "../bindings/JobSummary";
import type { Health } from "../bindings/Health";
import type { Conversation, JobChain } from "../model/jobList";
import { countSummary, hiddenChains, isActive, moreLabel, shownChains } from "../model/jobList";
import { modelLabel, elapsedJob, finishedTime, requestLead, stateName, workspaceName } from "../model/format";

type Row = { kind: "heading"; conversation: Conversation; open: boolean }
  | { kind: "job"; chain: JobChain }
  | { kind: "more"; conversation: Conversation; hidden: JobChain[]; open: boolean };

export function JobList({ conversations, expanded, expandedCompleted, selected, unseen, health, now, onSelect, onToggle, onToggleCompleted }: {
  conversations: Conversation[]; expanded: Record<string, boolean>; expandedCompleted: Record<string, boolean>;
  selected: string | null; unseen: string[]; health: Health; now: number;
  onSelect: (id: string) => void; onToggle: (key: string, open: boolean) => void; onToggleCompleted: (key: string) => void;
}) {
  const parent = useRef<HTMLDivElement>(null);
  const rows: Row[] = conversations.flatMap((conversation) => {
    const open = expanded[conversation.key] ?? conversation.defaultOpen;
    if (!open) return [{ kind: "heading", conversation, open }];
    const allCompleted = !!expandedCompleted[conversation.key];
    const hidden = hiddenChains(conversation, unseen);
    return [{ kind: "heading", conversation, open },
      ...shownChains(conversation, allCompleted, unseen).map((chain): Row => ({ kind: "job", chain })),
      ...(hidden.length ? [{ kind: "more" as const, conversation, hidden, open: allCompleted }] : [])];
  });
  const virtual = useVirtualizer({ count: rows.length, getScrollElement: () => parent.current,
    estimateSize: (index) => rows[index].kind === "heading" ? 55 : 42, overscan: 6 });
  useEffect(() => {
    const index = rows.findIndex((row) => row.kind === "job" && row.chain.latest.id === selected);
    if (index >= 0) virtual.scrollToIndex(index, { align: "auto" });
  }, [selected, expanded, expandedCompleted, conversations]);
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
            <span>{row.open ? "▼" : "▶"} {row.conversation.title}</span><small>{countSummary(row.conversation)}</small>
          </button> : row.kind === "more" ? <button type="button" className="more-jobs" onClick={() => onToggleCompleted(row.conversation.key)}>
            {row.open ? "畳む" : moreLabel(row.hidden)}</button>
            : <JobRow chain={row.chain} selected={selected === row.chain.latest.id} unseen={unseen.includes(row.chain.latest.id)} now={now} onSelect={onSelect} />}
        </div>;
      })}
    </div>
  </div>;
}

function JobRow({ chain, selected, unseen, now, onSelect }: { chain: JobChain; selected: boolean; unseen: boolean; now: number; onSelect: (id: string) => void }) {
  const job = chain.latest;
  const lead = requestLead(job);
  const tooltip = `${lead}\n${workspaceName(job)}${job.promptHead ? `\n${job.promptHead}` : ""}`;
  return <button type="button" role="option" aria-selected={selected} title={tooltip}
    className={`job-row ${selected ? "selected" : ""}`} onClick={() => onSelect(job.id)}>
    <span className={`state-icon ${job.effectiveState}`} aria-hidden="true">{icon(job)}</span>
    {unseen && <span className="unseen-dot" aria-label="未確認" />}
    <span className="job-label"><strong className="job-lead">{lead}</strong>
      {job.model && <small className="job-model" title={modelLabel(job) ?? undefined}>{modelLabel(job)}</small>}</span>
    {chain.previous.length > 0 && <small className="continuation">続き {chain.previous.length}</small>}
    {job.kindLabel && job.kindLabel !== "rescue" && <small className="job-kind">{job.kindLabel}</small>}
    <span className={`job-status ${isActive(job) ? "active" : ""}`}>
      {stateName(job.effectiveState, job.phase, job.status, job.archivedNote)} {isActive(job) ? elapsedJob(job, now) : finishedTime(job.completedAt ?? job.createdAt, now)}
    </span>
  </button>;
}
function icon(job: JobSummary) {
  switch (job.effectiveState) {
    case "queued": return "○"; case "running": case "quiet": return "●";
    case "orphaned": return "！"; case "completed": return "✓";
    case "failed": return "✕"; case "cancelled": return "－"; default: return "？";
  }
}
