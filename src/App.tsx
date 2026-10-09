import { useEffect, useMemo, useReducer, useState } from "react";
import { api } from "./api";
import { Banner } from "./components/Banner";
import { JobDetail } from "./components/JobDetail";
import { JobList } from "./components/JobList";
import { SettingsDialog } from "./components/SettingsDialog";
import { conversationKey, groups, visibleJobIds, workspaces } from "./model/jobList";
import { initialState, reducer } from "./model/store";
import "./styles.css";

export function App() {
  const [state, dispatch] = useReducer(reducer, initialState);
  const [workspace, setWorkspace] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>({});
  const [expandedChains, setExpandedChains] = useState<Record<string, boolean>>({});
  const [now, setNow] = useState(Date.now());
  const snapshot = state.snapshot;
  const list = useMemo(() => snapshot ? groups(snapshot.jobs, snapshot.settings, now, workspace, snapshot.unseen) : [], [snapshot, now, workspace]);
  const ids = visibleJobIds(list, expanded, expandedChains);
  const job = snapshot?.jobs.find((j) => j.id === state.selected) ?? null;
  useEffect(() => {
    if (!job) return;
    setExpanded((current) => ({ ...current, [conversationKey(job)]: true }));
    const chain = list.find((conversation) => conversation.key === conversationKey(job))?.chains.find((entry) => entry.previous.slice(3).some((previous) => previous.id === job.id));
    if (chain) setExpandedChains((current) => ({ ...current, [chain.latest.id]: true }));
  }, [state.selected]);

  useEffect(() => {
    let alive = true;
    const unlisteners: (() => void)[] = [];
    (async () => {
      try {
        const first = await api.snapshot(); if (!alive) return;
        dispatch({ type: "snapshot", value: first });
        const pending = await api.takePendingFocus(); if (!alive) return;
        if (pending) dispatch({ type: "select", id: pending });
        const listeners = await Promise.all([
          api.onMonitor((value) => dispatch({ type: "monitor", value })),
          api.onUnseen((value) => dispatch({ type: "unseen", value })),
          api.onSettings((value) => dispatch({ type: "settings", value })),
          api.onFocus((id) => dispatch({ type: "select", id })),
        ]);
        unlisteners.push(...listeners);
        if (!alive) { listeners.forEach((stop) => stop()); return; }
        dispatch({ type: "snapshot", value: await api.snapshot() });
      } catch (error) { if (alive) dispatch({ type: "error", value: String(error) }); }
    })();
    return () => { alive = false; unlisteners.forEach((stop) => stop()); };
  }, []);
  useEffect(() => { const timer = setInterval(() => setNow(Date.now()), 30_000); return () => clearInterval(timer); }, []);
  useEffect(() => {
    if (!state.selected) { void api.watchLog(null); return; }
    let alive = true;
    const id = state.selected;
    dispatch({ type: "error", value: null });
    Promise.all([api.job(id), job?.archived ? Promise.resolve(null) : api.log(id, null), api.watchLog(job?.archived ? null : id), api.markSeen(id)])
      .then(([detail, log]) => { if (alive) { dispatch({ type: "detail", value: detail }); dispatch({ type: "log", value: log }); } })
      .catch((error) => { if (alive) dispatch({ type: "error", value: String(error) }); });
    return () => { alive = false; };
  }, [state.selected, job?.archived]);
  useEffect(() => {
    if (state.logNeedsReload && state.selected) {
      api.log(state.selected, null).then((value) => dispatch({ type: "log", value }))
        .catch((error) => dispatch({ type: "error", value: String(error) }));
    }
  }, [state.logNeedsReload, state.selected]);
  useEffect(() => {
    if (!state.selected || !job) return;
    let alive = true;
    api.job(state.selected).then((detail) => { if (alive) dispatch({ type: "detail", value: detail }); })
      .catch((error) => { if (alive) dispatch({ type: "error", value: String(error) }); });
    return () => { alive = false; };
  }, [state.selected, job?.updatedAt, job?.effectiveState, job?.summary]);
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
  function onKeyDown(event: React.KeyboardEvent) {
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
    {snapshot && <Banner health={snapshot.health} />}
    <div className="layout"><aside className="sidebar"><div className="toolbar">
      <select aria-label="ワークスペース" value={workspace ?? ""} onChange={(e) => setWorkspace(e.target.value || null)}>
        <option value="">すべて</option>{snapshot && workspaces(snapshot.jobs).map((name) => <option value={name} key={name}>{name}</option>)}
      </select><button title="設定" aria-label="設定" onClick={() => setSettingsOpen(true)}>⚙</button></div>
      {snapshot ? <JobList conversations={list} expanded={expanded} expandedChains={expandedChains} selected={state.selected} unseen={snapshot.unseen} health={snapshot.health} now={now} onSelect={select}
        onToggle={(key, open) => setExpanded((current) => ({ ...current, [key]: open }))}
        onToggleChain={(id) => setExpandedChains((current) => ({ ...current, [id]: !current[id] }))} />
        : <div className="loading">読み込み中…</div>}
    </aside><main className="main-panel"><JobDetail job={job} jobs={snapshot?.jobs ?? []} detail={state.detail} log={state.log} now={now} error={state.error}
      onOlder={older} onOpen={openPath} onCopy={copy} onSelect={select} /></main></div>
    {settingsOpen && snapshot && <SettingsDialog settings={snapshot.settings} onClose={() => setSettingsOpen(false)} onSave={async (value) => {
      const saved = await api.setSettings(value); dispatch({ type: "settings", value: saved });
    }} />}
  </div>;
}
