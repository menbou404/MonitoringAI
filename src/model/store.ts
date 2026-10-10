import type { UiSnapshot } from "../bindings/UiSnapshot";
import type { JobDetail } from "../bindings/JobDetail";
import type { LogChunk } from "../bindings/LogChunk";
import type { MonitorEvent } from "../bindings/MonitorEvent";
import type { Settings } from "../bindings/Settings";

export type ViewState = { snapshot: UiSnapshot | null; selected: string | null; detail: JobDetail | null;
  log: LogChunk | null; logNeedsReload: boolean; error: string | null };
export const initialState: ViewState = { snapshot: null, selected: null, detail: null, log: null, logNeedsReload: false, error: null };
export type Action = { type: "snapshot"; value: UiSnapshot } | { type: "monitor"; value: MonitorEvent }
  | { type: "unseen"; value: string[] } | { type: "settings"; value: Settings }
  | { type: "select"; id: string | null } | { type: "detail"; value: JobDetail | null }
  | { type: "log"; value: LogChunk | null } | { type: "prepend"; value: LogChunk }
  | { type: "error"; value: string | null };
export function reducer(state: ViewState, action: Action): ViewState {
  switch (action.type) {
    case "snapshot": return { ...state, snapshot: action.value,
      selected: state.selected && action.value.jobs.some((j) => j.id === state.selected) ? state.selected : null,
      detail: state.selected && action.value.jobs.some((j) => j.id === state.selected) ? state.detail : null };
    case "select": return state.selected === action.id
      ? state
      : { ...state, selected: action.id, detail: null, log: null, logNeedsReload: false, error: null };
    case "detail": return { ...state, detail: action.value };
    case "log": return { ...state, log: action.value, logNeedsReload: false };
    case "prepend": return state.log && action.value.end === state.log.start
      ? { ...state, log: { ...state.log, text: action.value.text + state.log.text, start: action.value.start, truncatedBefore: action.value.truncatedBefore } }
      : { ...state, logNeedsReload: true };
    case "error": return { ...state, error: action.value };
    case "unseen": return state.snapshot ? { ...state, snapshot: { ...state.snapshot, unseen: action.value } } : state;
    case "settings": return state.snapshot ? { ...state, snapshot: { ...state.snapshot, settings: action.value } } : state;
    case "monitor": {
      if (!state.snapshot) return state;
      const event = action.value;
      if (event.type === "jobsChanged") {
        const map = new Map(state.snapshot.jobs.map((j) => [j.id, j]));
        event.removed.forEach((id) => map.delete(id));
        event.changed.forEach((j) => map.set(j.id, j));
        const selected = state.selected && map.has(state.selected) ? state.selected : null;
        return { ...state, snapshot: { ...state.snapshot, jobs: [...map.values()] }, selected,
          detail: selected ? state.detail : null, log: selected ? state.log : null };
      }
      if (event.type === "healthChanged") return { ...state, snapshot: { ...state.snapshot, health: event } };
      if (event.type === "logAppended" && event.id === state.selected && state.log) {
        if (event.start !== state.log.end) return { ...state, logNeedsReload: true };
        return { ...state, log: { ...state.log, text: state.log.text + event.text, end: event.end } };
      }
      return state;
    }
  }
}
