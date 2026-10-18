import type { JobDetail as Detail } from "../bindings/JobDetail";
import type { JobSummary } from "../bindings/JobSummary";
import type { LogChunk } from "../bindings/LogChunk";
import { modelLabel, clock, conversationTitle, elapsed, elapsedJob, requestLead, startMs, stateName, workspaceName } from "../model/format";
import { LogView } from "./LogView";

export function JobDetail({ job, jobs, detail, log, now, error, onOlder, onOpen, onCopy, onSelect }: {
  job: JobSummary | null; jobs: JobSummary[]; detail: Detail | null; log: LogChunk | null; now: number; error: string | null;
  onOlder: () => void; onOpen: (kind: "workspace" | "log") => void; onCopy: (value: string) => void; onSelect: (id: string) => void;
}) {
  if (!job) return <section className="detail-placeholder">{error ? <div className="detail-error" role="alert">{error}</div> : "左の一覧からジョブを選んでください"}</section>;
  const finished = ["completed", "failed", "cancelled"].includes(job.effectiveState);
  const end = job.completedAt ?? job.createdAt;
  const duration = finished && end ? elapsed(Math.max(0, (Date.parse(end) - startMs(job)) / 1000)) : elapsedJob(job, now);
  const previous = jobs.find((candidate) => candidate.id === job.previousJobId);
  const next = jobs.find((candidate) => candidate.previousJobId === job.id);
  const requester = job.context?.requester;
  return <section className="detail-panel">
    {error && <div className="detail-error" role="alert">{error}</div>}
    {job.archived && <div className="archive-notice">codex-plugin-cc の一覧から消えたジョブです（Claude の会話が終わると消えます）。ログは表示できません。
      {job.archivedNote && <p>{job.archivedNote}</p>}</div>}
    <header className="detail-header"><div className="detail-title"><h1>{requestLead(job)}</h1>
      <span className={`status ${job.effectiveState}`}>{stateName(job.effectiveState, job.phase, job.status, job.archivedNote)}</span></div>
      <div className="detail-context">{conversationTitle(job)} · {workspaceName(job)} · {job.kindLabel || job.kind || "種別不明"}</div>
      {job.model && <div className="detail-context">モデル: {modelLabel(job)}</div>}
      {requester && <div className="detail-context">{requester.kind === "main" ? "会話から直接依頼" : requester.parentDescription ? `${requester.parentDescription} から依頼` : "サブエージェントから依頼"}</div>}
      <p>開始 {clock(job.startedAt ?? job.createdAt)} · {finished ? `所要 ${duration} · 終了 ${clock(end)}` : duration}{job.write && " · 書き込みあり"}</p></header>
    {(previous || next) && <nav className="continuity" aria-label="地続きのジョブ">
      {previous && <button onClick={() => onSelect(previous.id)}>前回: {requestLead(previous)}</button>}
      {next && <button onClick={() => onSelect(next.id)}>次回: {requestLead(next)}</button>}
    </nav>}
    <details className="prompt"><summary>{job.archived ? "依頼文の先頭" : "依頼の全文"}</summary><div>{job.archived ? job.promptHead || "依頼文はありません" : detail?.prompt || "依頼の全文はありません"}</div></details>
    {finished && <section className="result"><div className="section-heading">結果</div>
      {job.effectiveState === "completed" ? <><p>{job.summary}</p>{detail?.rendered && <pre>{detail.rendered}</pre>}</>
        : <p>{detail?.errorMessage || "詳細はありません"}</p>}</section>}
    {!job.archived && <LogView log={log} onOlder={onOlder} />}
    <div className="detail-actions"><button onClick={() => onOpen("workspace")} disabled={!job.workspaceRoot}>フォルダを開く</button>
      {!job.archived && <button onClick={() => onOpen("log")}>ログを開く</button>}
      <button onClick={() => onCopy(job.id)}>ジョブIDをコピー</button>
      {job.threadId && <button onClick={() => onCopy(job.threadId!)}>threadIdをコピー</button>}</div>
  </section>;
}
