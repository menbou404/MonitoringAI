import { useEffect, useRef } from "react";
import type { JobSummary } from "../bindings/JobSummary";
import { conversationTitle, elapsedJob, requestLead, stateName } from "../model/format";
import { completionTime, movementAgo, recentCompleted, runningWorkspaces, shortLatestMessage } from "../model/running";

export function RunningView({ jobs, unseen, now, listDays, onOpen, onAll }: {
  jobs: JobSummary[]; unseen: string[]; now: number; listDays: number; onOpen: (id: string) => void; onAll: () => void;
}) {
  const workspaces = runningWorkspaces(jobs);
  const active = workspaces.flatMap((workspace) => workspace.jobs);
  const recent = recentCompleted(jobs, now, listDays);
  const cardRefs = useRef<(HTMLButtonElement | null)[]>([]);
  useEffect(() => { cardRefs.current.length = active.length; }, [active.length]);
  function navigate(event: React.KeyboardEvent<HTMLButtonElement>, index: number) {
    if (!["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
    event.preventDefault();
    const forward = event.key === "ArrowDown" || event.key === "ArrowRight";
    cardRefs.current[Math.max(0, Math.min(active.length - 1, index + (forward ? 1 : -1)))]?.focus();
  }
  return <main className="running-view">
    {!active.length && <p className="running-empty">実行中のCodexジョブはありません</p>}
    {workspaces.map((workspace) => <section className="running-workspace" key={workspace.root ?? "__unknown__"}>
      <h2 className="running-workspace-heading" title={workspace.root ?? undefined}><span aria-hidden="true">📁</span>
        <span className="running-workspace-name">{workspace.name}</span>
        <span className="running-workspace-count">実行中 {workspace.activeCount}</span>
        {workspace.attentionCount > 0 && <span className="running-workspace-attention">要確認 {workspace.attentionCount}</span>}
      </h2>
      <div className="running-cards">{workspace.jobs.map((job) => {
        const index = active.findIndex((entry) => entry.id === job.id);
        const movement = movementAgo(job.logModifiedAt, now);
        return <button key={job.id} type="button" className={`running-card ${job.effectiveState}`}
          ref={(element) => { cardRefs.current[index] = element; }} onKeyDown={(event) => navigate(event, index)} onClick={() => onOpen(job.id)}>
          <span className="running-card-top"><span className={`state-icon ${job.effectiveState}`} aria-hidden="true">{job.effectiveState === "orphaned" ? "！" : job.effectiveState === "queued" ? "○" : "●"}</span>
            {unseen.includes(job.id) && <span className="unseen-dot" aria-label="未確認" />}
            <strong>{requestLead(job)}</strong></span>
          <span className="running-card-conversation">{conversationTitle(job)}</span>
          <span className="running-card-meta"><span className={`status ${job.effectiveState}`}>{stateName(job.effectiveState, job.phase, job.status, job.archivedNote)}</span>
            <span>{elapsedJob(job, now)}{movement && ` · ${movement}`}</span></span>
          {job.latestMessage && <span className="running-message" title={job.latestMessage}>{shortLatestMessage(job.latestMessage)}</span>}
        </button>;
      })}</div>
    </section>)}
    {recent.length > 0 && (active.length > 0 ? <button type="button" className="recent-inline" onClick={onAll}>
      直近の完了: {requestLead(recent[0])}（{completionTime(recent[0])}）{recent.length > 1 && ` · ほか ${recent.length - 1}件`}
    </button> : <section className="recent-list"><h2>直近の完了</h2>{recent.slice(0, 3).map((job) => <button key={job.id} onClick={() => onOpen(job.id)}>
      <span>{requestLead(job)}</span><time>{completionTime(job)}</time></button>)}</section>)}
  </main>;
}
