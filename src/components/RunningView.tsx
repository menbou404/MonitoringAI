import { useRef, useState, type DragEvent } from "react";
import type { JobSummary } from "../bindings/JobSummary";
import { conversationTitle, elapsedJob, requestLead, stateName } from "../model/format";
import { completionTime, movementAgo, recentCompleted, reorderCards, reorderFolders, runningCardActivation,
  runningWorkspaces, shortLatestMessage } from "../model/running";

type DragSource = { kind: "folder" | "card"; folderKey: string; id: string };
type DropTarget = { kind: "folder" | "card"; id: string; after: boolean };

export function RunningView({ jobs, unseen, now, listDays, folderOrder, cardOrder, onOpen, onAll, onReorderFolders, onReorderCards, onReset }: {
  jobs: JobSummary[]; unseen: string[]; now: number; listDays: number; folderOrder: string[]; cardOrder: Record<string, string[]>;
  onOpen: (id: string) => void; onAll: () => void;
  onReorderFolders: (ids: string[]) => Promise<void>; onReorderCards: (folderKey: string, ids: string[]) => Promise<void>;
  onReset: () => Promise<void>;
}) {
  const workspaces = runningWorkspaces(jobs, folderOrder, cardOrder);
  const activeCount = workspaces.reduce((sum, workspace) => sum + workspace.activeCount, 0);
  const recent = recentCompleted(jobs, now, listDays);
  const drag = useRef<DragSource | null>(null);
  const suppressClick = useRef(false);
  const [drop, setDrop] = useState<DropTarget | null>(null);
  const [orderError, setOrderError] = useState<string | null>(null);
  const hasOrder = folderOrder.length > 0 || Object.values(cardOrder).some((ids) => ids.length > 0);

  function startDrag(event: DragEvent<HTMLElement>, source: DragSource) {
    drag.current = source;
    suppressClick.current = runningCardActivation(suppressClick.current, "dragstart").suppressClick;
    event.dataTransfer.effectAllowed = "move";
    event.dataTransfer.setData("text/plain", source.id);
  }
  function endDrag() {
    drag.current = null;
    setDrop(null);
    suppressClick.current = true;
  }
  function insertionAfter(event: DragEvent<HTMLElement>, axis: "vertical" | "horizontal"): boolean {
    const rect = event.currentTarget.getBoundingClientRect();
    return axis === "vertical" ? event.clientY >= rect.top + rect.height / 2
      : event.clientX >= rect.left + rect.width / 2;
  }
  async function commit(action: Promise<void>) {
    try { await action; setOrderError(null); }
    catch (error) { setOrderError(String(error)); }
  }
  function folderOver(event: DragEvent<HTMLElement>, target: string) {
    if (drag.current?.kind !== "folder") {
      if (drag.current?.kind === "card") {
        event.dataTransfer.dropEffect = "none";
        setDrop(null);
      }
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setDrop({ kind: "folder", id: target, after: insertionAfter(event, "vertical") });
  }
  function folderDrop(event: DragEvent<HTMLElement>, target: string) {
    if (drag.current?.kind !== "folder") return;
    event.preventDefault();
    const ids = reorderFolders(workspaces, drag.current.id, target, insertionAfter(event, "vertical"));
    endDrag();
    if (ids && ids.join("\0") !== workspaces.map((workspace) => workspace.key).join("\0"))
      void commit(onReorderFolders(ids));
  }
  function cardOver(event: DragEvent<HTMLElement>, folderKey: string, target: string) {
    if (drag.current?.kind !== "card") return;
    event.stopPropagation();
    if (drag.current.folderKey !== folderKey) {
      event.dataTransfer.dropEffect = "none";
      setDrop(null);
      return;
    }
    event.preventDefault();
    event.dataTransfer.dropEffect = "move";
    setDrop({ kind: "card", id: target, after: insertionAfter(event, "horizontal") });
  }
  function cardDrop(event: DragEvent<HTMLElement>, folderKey: string, target: string) {
    if (drag.current?.kind !== "card") return;
    event.stopPropagation();
    event.preventDefault();
    const source = drag.current;
    const ids = reorderCards(workspaces, source.folderKey, folderKey, source.id, target, insertionAfter(event, "horizontal"));
    const before = workspaces.find((workspace) => workspace.key === folderKey)?.jobs.map((job) => job.id);
    endDrag();
    if (ids && ids.join("\0") !== before?.join("\0")) void commit(onReorderCards(folderKey, ids));
  }

  return <main className="running-view" onDragOver={(event) => {
    if (!(event.target as HTMLElement).closest(".running-workspace")) setDrop(null);
  }}>
    {hasOrder && <button type="button" className="reset-running-order" onClick={() => void commit(onReset())}>並びを自動に戻す</button>}
    {orderError && <div className="detail-error" role="alert">{orderError}</div>}
    {!activeCount && <p className="running-empty">実行中のCodexジョブはありません</p>}
    <div className="running-workspaces">{workspaces.map((workspace) => {
      const folderInsertion = drop?.kind === "folder" && drop.id === workspace.key;
      return <section className={`running-workspace ${folderInsertion ? drop.after ? "insert-after" : "insert-before" : ""}`}
        key={workspace.key}
        onDragOver={(event) => folderOver(event, workspace.key)} onDrop={(event) => folderDrop(event, workspace.key)}>
        <h2 className="running-workspace-heading" title={workspace.root ?? undefined} draggable
          onDragStart={(event) => startDrag(event, { kind: "folder", folderKey: workspace.key, id: workspace.key })}
          onDragEnd={endDrag}><span aria-hidden="true">📁</span>
          <span className="running-workspace-name">{workspace.name}</span>
          <span className="running-workspace-count">実行中 {workspace.activeCount}</span>
          {workspace.attentionCount > 0 && <span className="running-workspace-attention">要確認 {workspace.attentionCount}</span>}
        </h2>
        <div className="running-cards">{workspace.jobs.map((job) => {
          const movement = movementAgo(job.logModifiedAt, now);
          const cardInsertion = drop?.kind === "card" && drop.id === job.id;
          return <div key={job.id} role="button" tabIndex={0} draggable title={job.latestMessage ?? undefined}
            className={`running-card ${job.effectiveState} ${cardInsertion ? drop.after ? "insert-after" : "insert-before" : ""}`}
            onMouseDown={() => { suppressClick.current = runningCardActivation(suppressClick.current, "mousedown").suppressClick; }}
            onDragStart={(event) => startDrag(event, { kind: "card", folderKey: workspace.key, id: job.id })}
            onDragEnd={endDrag} onDragOver={(event) => cardOver(event, workspace.key, job.id)}
            onDrop={(event) => cardDrop(event, workspace.key, job.id)}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                suppressClick.current = false;
                onOpen(job.id);
              }
            }}
            onClick={(event) => {
              const activation = runningCardActivation(suppressClick.current, "click");
              suppressClick.current = activation.suppressClick;
              if (!activation.open) { event.preventDefault(); return; }
              onOpen(job.id);
            }}>
            <span className="running-card-top"><span className={`state-icon ${job.effectiveState}`} aria-hidden="true">{job.effectiveState === "orphaned" ? "！" : job.effectiveState === "queued" ? "○" : "●"}</span>
              {unseen.includes(job.id) && <span className="unseen-dot" aria-label="未確認" />}
              <strong>{requestLead(job)}</strong></span>
            <span className="running-card-conversation">{conversationTitle(job)}</span>
            <span className="running-card-meta"><span className={`status ${job.effectiveState}`}>{stateName(job.effectiveState, job.phase, job.status, job.archivedNote)}</span>
              <span>{elapsedJob(job, now)}{movement && ` · ${movement}`}</span></span>
            {job.latestMessage && <span className="running-message" title={job.latestMessage}>{shortLatestMessage(job.latestMessage)}</span>}
          </div>;
        })}</div>
      </section>;
    })}</div>
    {recent.length > 0 && (activeCount > 0 ? <button type="button" className="recent-inline" onClick={onAll}>
      直近の完了: {requestLead(recent[0])}（{completionTime(recent[0])}）{recent.length > 1 && ` · ほか ${recent.length - 1}件`}
    </button> : <section className="recent-list"><h2>直近の完了</h2>{recent.slice(0, 3).map((job) => <button key={job.id} onClick={() => onOpen(job.id)}>
      <span>{requestLead(job)}</span><time>{completionTime(job)}</time></button>)}</section>)}
  </main>;

}
