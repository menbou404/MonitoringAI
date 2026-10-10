import { useEffect, useRef } from "react";
import type { JobSummary } from "../bindings/JobSummary";
import { conversationTitle, elapsedJob, requestLead, stateName, workspaceName } from "../model/format";
import { completionTime, movementAgo, recentCompleted, runningJobs } from "../model/running";

export function RunningView({ jobs, unseen, now, listDays, onOpen, onAll }: {
  jobs: JobSummary[]; unseen: string[]; now: number; listDays: number; onOpen: (id: string) => void; onAll: () => void;
}) {
  const active = runningJobs(jobs);
  const recent = recentCompleted(jobs, now, listDays);
  const cardRefs = useRef<(HTMLButtonElement | null)[]>([]);
  useEffect(() => { cardRefs.current.length = active.length; }, [active.length]);
  function navigate(event: React.KeyboardEvent<HTMLButtonElement>, index: number) {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    cardRefs.current[Math.max(0, Math.min(active.length - 1, index + (event.key === "ArrowDown" ? 1 : -1)))]?.focus();
  }
  return <main className="running-view">
    {!active.length && <p className="running-empty">実行中のCodexジョブはありません</p>}
    <div className="running-cards">{active.map((job, index) => <button key={job.id} type="button" className={`running-card ${job.effectiveState}`}
      ref={(element) => { cardRefs.current[index] = element; }} onKeyDown={(event) => navigate(event, index)} onClick={() => onOpen(job.id)}>
      <span className="running-card-top"><span className={`state-icon ${job.effectiveState}`} aria-hidden="true">{job.effectiveState === "orphaned" ? "！" : job.effectiveState === "queued" ? "○" : "●"}</span>
        {unseen.includes(job.id) && <span className="unseen-dot" aria-label="未確認" />}
        <strong>{requestLead(job)}</strong><span className={`status ${job.effectiveState}`}>{stateName(job.effectiveState, job.phase, job.status, job.archivedNote)}</span></span>
      <span className="running-card-meta"><span>{conversationTitle(job)} · {workspaceName(job)}</span>
        <span>{elapsedJob(job, now)}{movementAgo(job.logModifiedAt, now) && ` · ${movementAgo(job.logModifiedAt, now)}`}</span></span>
      {job.latestMessage && <span className="running-message">{job.latestMessage}</span>}
    </button>)}</div>
    {recent.length > 0 && (active.length > 0 ? <button type="button" className="recent-inline" onClick={onAll}>
      直近の完了: {requestLead(recent[0])}（{completionTime(recent[0])}）{recent.length > 1 && ` · ほか ${recent.length - 1}件`}
    </button> : <section className="recent-list"><h2>直近の完了</h2>{recent.slice(0, 3).map((job) => <button key={job.id} onClick={() => onOpen(job.id)}>
      <span>{requestLead(job)}</span><time>{completionTime(job)}</time></button>)}</section>)}
  </main>;
}
