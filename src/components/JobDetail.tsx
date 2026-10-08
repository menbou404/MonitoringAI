import type { JobDetail as Detail } from "../bindings/JobDetail";
import type { JobSummary } from "../bindings/JobSummary";
import type { LogChunk } from "../bindings/LogChunk";
import { clock, elapsed, elapsedJob, startMs, stateName, workspaceName } from "../model/format";
import { LogView } from "./LogView";

export function JobDetail({ job, detail, log, now, error, onOlder, onOpen, onCopy }: {
  job: JobSummary | null; detail: Detail | null; log: LogChunk | null; now: number; error: string | null;
  onOlder: () => void; onOpen: (kind: "workspace" | "log") => void; onCopy: (value: string) => void;
}) {
  if (!job) return <section className="detail-placeholder">{error ? <div className="detail-error" role="alert">{error}</div> : "左の一覧からジョブを選んでください"}</section>;
  const finished = ["completed", "failed", "cancelled"].includes(job.effectiveState);
  const end = job.completedAt ?? job.updatedAt;
  const duration = finished && end ? elapsed(Math.max(0, (Date.parse(end) - startMs(job)) / 1000)) : elapsedJob(job, now);
  return <section className="detail-panel">
    {error && <div className="detail-error" role="alert">{error}</div>}
    <header className="detail-header"><div className="detail-title"><h1>{workspaceName(job)}</h1>
      {job.kindLabel && <span className="kind">{job.kindLabel}</span>}
      <span className={`status ${job.effectiveState}`}>{stateName(job.effectiveState, job.phase, job.status)}</span></div>
      <p>開始 {clock(job.startedAt ?? job.createdAt)} · {finished ? `所要 ${duration} · 終了 ${clock(end)}` : duration}{job.write && " · 書き込みあり"}</p></header>
    <details className="prompt"><summary>依頼の全文</summary><div>{detail?.prompt || "依頼の全文はありません"}</div></details>
    {finished && <section className="result"><div className="section-heading">結果</div>
      {job.effectiveState === "completed" ? <><p>{job.summary}</p>{detail?.rendered && <pre>{detail.rendered}</pre>}</>
        : <p>{detail?.errorMessage || "詳細はありません"}</p>}</section>}
    <LogView log={log} onOlder={onOlder} />
    <div className="detail-actions"><button onClick={() => onOpen("workspace")} disabled={!job.workspaceRoot}>フォルダを開く</button>
      <button onClick={() => onOpen("log")}>ログを開く</button>
      <button onClick={() => onCopy(job.id)}>ジョブIDをコピー</button>
      {job.threadId && <button onClick={() => onCopy(job.threadId!)}>threadIdをコピー</button>}</div>
  </section>;
}
