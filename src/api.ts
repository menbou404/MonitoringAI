import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { UiSnapshot } from "./bindings/UiSnapshot";
import type { JobDetail } from "./bindings/JobDetail";
import type { LogChunk } from "./bindings/LogChunk";
import type { Settings } from "./bindings/Settings";
import type { ModelSwitchStatus } from "./bindings/ModelSwitchStatus";
import type { MonitorEvent } from "./bindings/MonitorEvent";

import type { ModelAgent } from "./bindings/ModelAgent";
import type { ModelCombination } from "./bindings/ModelCombination";
import type { AgentModelSettings } from "./bindings/AgentModelSettings";

export const api = {
  snapshot: () => invoke<UiSnapshot>("get_snapshot"),
  job: (id: string) => invoke<JobDetail | null>("get_job", { id }),
  log: (id: string, from: number | null) => invoke<LogChunk | null>("read_log", { id, from }),
  watchLog: (id: string | null) => invoke<void>("watch_log", { id }),
  markSeen: (id: string) => invoke<void>("mark_seen", { id }),
  openPath: (kind: "workspace" | "log", id: string) => invoke<void>("open_path", { kind, id }),
  switchModel: (agent: ModelAgent, target: ModelCombination) => invoke<ModelSwitchStatus>("switch_model", { agent, target }),
  setModelSettings: (agent: ModelAgent, settings: AgentModelSettings) => invoke<Settings>("set_model_settings", { agent, settings }),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
  setCardOrder: (folderKey: string, ids: string[]) => invoke<void>("set_card_order", { folderKey, ids }),
  resetRunningOrder: () => invoke<Settings>("reset_running_order"),
  takePendingFocus: () => invoke<string | null>("take_pending_focus"),
  onMonitor: (fn: (event: MonitorEvent) => void): Promise<UnlistenFn> =>
    listen<MonitorEvent>("monitor-event", (event) => fn(event.payload)),
  onModelSwitch: (fn: (status: ModelSwitchStatus) => void): Promise<UnlistenFn> =>
    listen<ModelSwitchStatus>("model-switch-changed", (event) => fn(event.payload)),
  onUnseen: (fn: (ids: string[]) => void): Promise<UnlistenFn> =>
    listen<string[]>("unseen-changed", (event) => fn(event.payload)),
  onSettings: (fn: (settings: Settings) => void): Promise<UnlistenFn> =>
    listen<Settings>("settings-changed", (event) => fn(event.payload)),
  onFocus: (fn: (id: string) => void): Promise<UnlistenFn> =>
    listen<{ id: string }>("focus-job", (event) => fn(event.payload.id)),
};
