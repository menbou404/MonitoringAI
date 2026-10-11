import { useEffect, useMemo, useReducer, useState } from "react";
import { api } from "./api";
import { Banner } from "./components/Banner";
import { JobDetail } from "./components/JobDetail";
import { JobList } from "./components/JobList";
import { RunningView } from "./components/RunningView";
import { SettingsDialog } from "./components/SettingsDialog";
import { conversationKey, groups, shownChains, visibleJobIds, workspaces } from "./model/jobList";
import { initialState, reducer } from "./model/store";
import "./styles.css";

export function App() {
  const [state, dispatch] = useReducer(reducer, initialState);
  const [workspace, setWorkspace] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [expandedCompleted, setExpandedCompleted] = useState<Record<string, boolean>>({});
  const [now, setNow] = useState(Date.now());
  const snapshot = state.snapshot;
  const list = useMemo(() => snapshot ? groups(snapshot.jobs, snapshot.settings, now, workspace, snapshot.unseen) : [], [snapshot, now, workspace]);
  const ids = visibleJobIds(list, expanded, expandedCompleted, snapshot?.unseen ?? []);
  const job = snapshot?.jobs.find((j) => j.id === state.selected) ?? null;
  useEffect(() => {
    if (!job) return;
    setExpanded((current) => ({ ...current, [conversationKey(job)]: true }));
    const conversation = list.find((entry) => entry.key === conversationKey(job));
    if (conversation && conversation.chains.some((entry) => entry.latest.id === job.id)
      && !shownChains(conversation, false, snapshot?.unseen ?? []).some((entry) => entry.latest.id === job.id))
      setExpandedCompleted((current) => ({ ...current, [conversation.key]: true }));
  }, [state.selected]);

  useEffect(() => {
    let alive = true;
    const unlisteners: (() => void)[] = [];
    (async () => {
      try {
        const first = await api.snapshot(); if (!alive) return;
        dispatch({ type: "snapshot", value: first });
        const pending = await api.takePendingFocus(); if (!alive) return;
        if (pending) dispatch({ type: "openJob", id: pending });
        const listeners = await Promise.all([
          api.onMonitor((value) => dispatch({ type: "monitor", value })),
          api.onUnseen((value) => dispatch({ type: "unseen", value })),
          api.onSettings((value) => dispatch({ type: "settings", value })),
          api.onFocus((id) => { setWorkspace(null); dispatch({ type: "openJob", id }); }),
        ]);
        unlisteners.push(...listeners);
        if (!alive) { listeners.forEach((stop) => stop()); return; }
        dispatch({ type: "snapshot", value: await api.snapshot() });
      } catch (error) { if (alive) dispatch({ type: "error", value: String(error) }); }
    })();
    return () => { alive = false; unlisteners.forEach((stop) => stop()); };
  }, []);
  useEffect(() => { setNow(Date.now()); const timer = setInterval(() => setNow(Date.now()), state.view === "running" ? 5_000 : 30_000); return () => clearInterval(timer); }, [state.view]);
  useEffect(() => {
    if (!state.selected || state.view !== "all") { void api.watchLog(null); return; }
    let alive = true;
    const id = state.selected;
    dispatch({ type: "error", value: null });
    Promise.all([api.job(id), job?.archived ? Promise.resolve(null) : api.log(id, null), api.watchLog(job?.archived ? null : id), api.markSeen(id)])
      .then(([detail, log]) => { if (alive) { dispatch({ type: "detail", value: detail }); dispatch({ type: "log", value: log }); } })
      .catch((error) => { if (alive) dispatch({ type: "error", value: String(error) }); });
    return () => { alive = false; };
  }, [state.selected, state.view, job?.archived]);
  useEffect(() => {
    if (state.logNeedsReload && state.selected && state.view === "all") {
      api.log(state.selected, null).then((value) => dispatch({ type: "log", value }))
        .catch((error) => dispatch({ type: "error", value: String(error) }));
    }
  }, [state.logNeedsReload, state.selected, state.view]);
  useEffect(() => {
    if (!state.selected || !job || state.view !== "all") return;
    let alive = true;
    api.job(state.selected).then((detail) => { if (alive) dispatch({ type: "detail", value: detail }); })
      .catch((error) => { if (alive) dispatch({ type: "error", value: String(error) }); });
    return () => { alive = false; };
  }, [state.selected, state.view, job?.updatedAt, job?.effectiveState, job?.summary]);
  async function older() {
    if (!state.selected || !state.log || !state.log.truncatedBefore) return;
    try {
      const oldStart = state.log.start;
      const chunk = await api.log(state.selected, Math.max(0, oldStart - 1_048_576));
      if (!chunk) return;
      const bytes = new TextEncoder().encode(chunk.text);
      const text = new TextDecoder().decode(bytes.slice(0, oldStart - chunk.start));
      dispatch({ type: "prepend", value: { text, start: chunk.start, end: oldStart, truncatedBefore: chunk.start > 0 } });
    } catch (error) { dispatch({ type: "error", value: String(error) }); }
  }
  function select(id: string) { dispatch({ type: "select", id }); }
  function openJob(id: string) { setWorkspace(null); dispatch({ type: "openJob", id }); }
  function onKeyDown(event: React.KeyboardEvent) {
    if (state.view !== "all") return;
    if (!["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight"].includes(event.key)) return;
    if ((event.target as HTMLElement).closest("input,textarea,select,dialog")) return;
    if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
      if (!job) return;
      event.preventDefault();
      setExpanded((current) => ({ ...current, [conversationKey(job)]: event.key === "ArrowRight" }));
      return;
    }
    if (!ids.length) return;
    event.preventDefault();
    const current = ids.indexOf(state.selected ?? "");
    const next = event.key === "ArrowDown" ? Math.min(ids.length - 1, current + 1) : Math.max(0, current < 0 ? 0 : current - 1);
    select(ids[next]);
  }
  async function openPath(kind: "workspace" | "log") {
    if (!state.selected) return;
    try { await api.openPath(kind, state.selected); } catch (error) { dispatch({ type: "error", value: String(error) }); }
  }
  async function copy(value: string) {
    try { await navigator.clipboard.writeText(value); } catch (error) { dispatch({ type: "error", value: String(error) }); }
  }
  return <div className="app" onKeyDown={onKeyDown}>
    <div className="topbar"><nav className="view-switch" aria-label="画面の切り替え">
      <button type="button" aria-current={state.view === "running" ? "page" : undefined} onClick={() => dispatch({ type: "view", view: "running" })}>
        実行中 {snapshot?.jobs.filter((job) => ["queued", "running", "quiet", "orphaned"].includes(job.effectiveState)).length ?? 0}</button>
      <button type="button" aria-current={state.view === "all" ? "page" : undefined} onClick={() => dispatch({ type: "view", view: "all" })}>すべて</button>
    </nav><div className="topbar-actions">{state.view === "all" && <select aria-label="ワークスペース" value={workspace ?? ""} onChange={(e) => setWorkspace(e.target.value || null)}>
      <option value="">すべてのワークスペース</option>{snapshot && workspaces(snapshot.jobs).map((name) => <option value={name} key={name}>{name}</option>)}
    </select>}<button title="設定" aria-label="設定" onClick={() => setSettingsOpen(true)}>⚙</button></div></div>
    {snapshot && <Banner health={snapshot.health} />}
    {state.view === "running" ? (snapshot ? <RunningView jobs={snapshot.jobs} unseen={snapshot.unseen} now={now} listDays={snapshot.settings.list_days}
      folderOrder={snapshot.settings.runningFolderOrder} cardOrder={snapshot.cardOrder}
      onReorderFolders={async (order) => { const saved = await api.setSettings({ ...snapshot.settings, runningFolderOrder: order }); dispatch({ type: "settings", value: saved }); }}
      onReorderCards={async (folderKey, order) => { await api.setCardOrder(folderKey, order); dispatch({ type: "cardOrder", folderKey, ids: order }); }}
      onReset={async () => { const saved = await api.resetRunningOrder(); dispatch({ type: "resetRunningOrder", settings: saved }); }}
      onOpen={openJob} onAll={() => { setWorkspace(null); dispatch({ type: "view", view: "all" }); }} /> : <div className="loading">読み込み中…</div>)
      : <div className="layout"><aside className="sidebar">
      {snapshot ? <JobList conversations={list} expanded={expanded} expandedCompleted={expandedCompleted} selected={state.selected} unseen={snapshot.unseen} health={snapshot.health} now={now} onSelect={select}
        onToggle={(key, open) => setExpanded((current) => ({ ...current, [key]: open }))}
        onToggleCompleted={(key) => setExpandedCompleted((current) => ({ ...current, [key]: !current[key] }))} />
        : <div className="loading">読み込み中…</div>}
    </aside><main className="main-panel"><JobDetail job={job} jobs={snapshot?.jobs ?? []} detail={state.detail} log={state.log} now={now} error={state.error}
      onOlder={older} onOpen={openPath} onCopy={copy} onSelect={select} /></main></div>}
    {settingsOpen && snapshot && <SettingsDialog settings={snapshot.settings} onClose={() => setSettingsOpen(false)} onSave={async (value) => {
      const saved = await api.setSettings(value); dispatch({ type: "settings", value: saved });
    }} />}
  </div>;
}
