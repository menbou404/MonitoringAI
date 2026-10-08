import { describe, expect, it } from "vitest";
import type { JobSummary } from "../bindings/JobSummary";
import type { Settings } from "../bindings/Settings";
import type { UiSnapshot } from "../bindings/UiSnapshot";
import { elapsed, phaseName, stateName } from "./format";
import { groups } from "./jobList";
import { parseLog } from "./logParse";
import { initialState, reducer } from "./store";

const now = Date.parse("2026-10-08T12:00:00Z");
const settings: Settings = { notifications: { enabled: true, started: false, completed: true, failed: true, orphaned: true, quiet: false },
  quiet_minutes: 10, recent_hours: 24, recent_limit: 10, extra_state_roots: [], autostart: false, window: null };
function job(id: string, state: JobSummary["effectiveState"], date = "2026-10-08T11:00:00Z"): JobSummary {
  return { id, workspaceRoot: "C:\\Shared", workspaceName: "Shared", kind: "task", kindLabel: "rescue", title: "Task", summary: `Request ${id}`,
    status: state, phase: null, effectiveState: state, createdAt: date, startedAt: date, completedAt: state === "completed" ? date : null,
    updatedAt: date, pid: null, threadId: null, write: false, sessionId: null, latestMessage: null, logModifiedAt: null };
}
const snapshot: UiSnapshot = { jobs: [], unseen: [], settings, health: { pluginVersion: "1.0.6", versionSupported: true, watchedRoots: [], lastError: null } };
describe("format", () => {
  it("uses the Rust phase labels, preserves unknown phases and states, and handles elapsed boundaries", () => {
    expect(phaseName("verifying")).toBe("検証中");
    expect(phaseName("future")).toBe("future");
    expect(stateName("quiet", null)).toBe("応答なし？");
    expect(stateName("unknown", null, "future-state")).toBe("future-state");
    expect(elapsed(59)).toBe("1分未満"); expect(elapsed(60)).toBe("1分"); expect(elapsed(3600)).toBe("1時間0分");
  });
});
describe("job groups", () => {
  it("keeps simultaneous jobs in one workspace distinct, sorts them, filters, and limits recent jobs", () => {
    const older = job("older", "running", "2026-10-08T10:00:00Z");
    const newer = job("newer", "running", "2026-10-08T11:00:00Z");
    const recent = job("recent", "completed", "2026-10-08T09:00:00Z");
    const old = job("expired", "completed", "2026-10-06T09:00:00Z");
    const other = { ...job("other", "running"), workspaceName: "Other" };
    const result = groups([older, newer, recent, old, other], settings, now, "Shared");
    expect(result.active.map((j) => j.id)).toEqual(["newer", "older"]);
    expect(result.recent.map((j) => j.id)).toEqual(["recent"]);
    expect(groups([recent, job("second", "completed", "2026-10-08T08:00:00Z")], { ...settings, recent_limit: 1 }, now, null).recent).toHaveLength(1);
  });
});
describe("log parsing", () => {
  it("recognizes lines, commands, failures, multiline blocks, and an incomplete last line", () => {
    const parsed = parseLog("[2026-10-08T10:00:00Z] Thread ready\n[2026-10-08T10:00:01Z] Running command: cargo test\n[2026-10-08T10:00:02Z] Command failed: cargo test\n[2026-10-08T10:00:03Z] Assistant message\n\nfirst\nsecond\n[2026-10-08T10:00:04Z] Final output\nend");
    expect(parsed.map((entry) => entry.kind)).toEqual(["line", "command", "failed", "block", "block"]);
    expect(parsed[3].body).toContain("first\nsecond");
    expect(parsed[4].body).toBe("end");
  });
});
describe("store", () => {
  it("applies every monitor event, unseen changes, deletion and log continuity", () => {
    let state = reducer(initialState, { type: "snapshot", value: snapshot });
    state = reducer(state, { type: "monitor", value: { type: "jobsChanged", changed: [job("one", "running")], removed: [] } });
    expect(state.snapshot?.jobs).toHaveLength(1);
    state = reducer(state, { type: "select", id: "one" });
    state = reducer(state, { type: "unseen", value: ["one"] });
    expect(state.snapshot?.unseen).toEqual(["one"]);
    state = reducer(state, { type: "log", value: { text: "abc", start: 0, end: 3, truncatedBefore: false } });
    state = reducer(state, { type: "monitor", value: { type: "logAppended", id: "one", text: "def", start: 3, end: 6 } });
    expect(state.log?.text).toBe("abcdef");
    state = reducer(state, { type: "monitor", value: { type: "logAppended", id: "one", text: "x", start: 9, end: 10 } });
    expect(state.logNeedsReload).toBe(true);
    state = reducer(state, { type: "monitor", value: { type: "healthChanged", pluginVersion: null, versionSupported: false, watchedRoots: [], lastError: "error" } });
    expect(state.snapshot?.health.lastError).toBe("error");
    state = reducer(state, { type: "monitor", value: { type: "stateTransition", id: "one", from: "running", to: "completed" } });
    state = reducer(state, { type: "monitor", value: { type: "jobsChanged", changed: [], removed: ["one"] } });
    expect(state.selected).toBeNull();
  });
});
