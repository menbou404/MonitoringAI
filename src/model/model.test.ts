import { describe, expect, it } from "vitest";
import type { JobSummary } from "../bindings/JobSummary";
import type { Settings } from "../bindings/Settings";
import type { UiSnapshot } from "../bindings/UiSnapshot";
import { elapsed, finishedTime, phaseName, requestLead, stateName } from "./format";
import { groups, visibleJobIds } from "./jobList";
import { parseLog } from "./logParse";
import { initialState, reducer } from "./store";

const now = Date.parse("2026-10-08T12:00:00Z");
const settings: Settings = { notifications: { enabled: true, started: false, completed: true, failed: true, orphaned: true, quiet: false },
  quiet_minutes: 10, list_days: 3, history_days: 30, extra_state_roots: [], autostart: false, window: null };
function job(id: string, state: JobSummary["effectiveState"], date = "2026-10-08T11:00:00Z"): JobSummary {
  return { id, workspaceRoot: "C:\\Shared", workspaceName: "Shared", kind: "task", kindLabel: "rescue", title: "Task", summary: `Request ${id}`,
    status: state, phase: null, effectiveState: state, createdAt: date, startedAt: date, completedAt: state === "completed" ? date : null,
    updatedAt: date, pid: null, threadId: null, write: false, sessionId: null, latestMessage: null, logModifiedAt: null,
    context: null, previousJobId: null, archived: false, archivedNote: null, promptHead: null };
}
const snapshot: UiSnapshot = { jobs: [], unseen: [], settings, health: { pluginVersion: "1.0.6", versionSupported: true, watchedRoots: [], lastError: null } };
describe("format", () => {
  it("uses the Rust phase labels, preserves unknown phases and states, and handles elapsed boundaries", () => {
    expect(phaseName("verifying")).toBe("検証中");
    expect(phaseName("future")).toBe("future");
    expect(stateName("quiet", null)).toBe("応答なし？");
    expect(stateName("unknown", null, "future-state")).toBe("future-state");
    expect(elapsed(59)).toBe("1分未満"); expect(elapsed(60)).toBe("1分"); expect(elapsed(3600)).toBe("1時間0分");
    expect(finishedTime("2026-10-07T09:00:00Z", now)).toMatch(/10\/07/);
  });
});
describe("job groups", () => {
  const context = (title: string, description: string | null) => ({ sessionId: title, sessionTitle: title,
    requester: { kind: "subagent" as const, description, agentType: null, depth: 1, parentDescription: null } });
  it("groups by conversation, orders active conversations and distinct requests, and falls back to prompt and title", () => {
    const older = { ...job("older", "running", "2026-10-08T10:00:00Z"), context: context("会話A", "画面を作る") };
    const newer = { ...job("newer", "running", "2026-10-08T11:00:00Z"), context: context("会話A", "設定を作る") };
    const other = { ...job("other", "running", "2026-10-08T09:00:00Z"), workspaceName: "Other", context: context("会話B", "別件") };
    const unknown = job("unknown", "completed");
    const result = groups([older, newer, other, unknown], settings, now, null);
    expect(result.map((group) => group.title)).toEqual(["会話A", "会話B", "会話が不明 — Shared"]);
    expect(result[0].chains.map((chain) => chain.latest.id)).toEqual(["newer", "older"]);
    expect(result[0].defaultOpen).toBe(true);
    expect(result[2].defaultOpen).toBe(false);
    expect(groups([unknown], settings, now, null, ["unknown"])[0].defaultOpen).toBe(true);
    expect(requestLead(newer)).toBe("設定を作る");
    expect(requestLead({ ...newer, context: context("会話A", null), promptHead: "依頼の先頭" })).toBe("依頼の先頭");
    expect(requestLead({ ...newer, context: null, promptHead: null })).toBe("Task");
    expect(groups([older, newer, other], settings, now, "Shared")).toHaveLength(1);
  });
  it("puts a three-job thread beneath its newest job and applies the display period", () => {
    const first = { ...job("first", "completed", "2026-10-07T08:00:00Z"), context: context("会話", "最初") };
    const second = { ...job("second", "completed", "2026-10-07T09:00:00Z"), context: context("会話", "続き"), previousJobId: "first" };
    const third = { ...job("third", "running", "2026-10-08T10:00:00Z"), context: context("会話", "最新"), previousJobId: "second" };
    const expired = job("expired", "completed", "2026-10-04T09:00:00Z");
    const result = groups([first, second, third, expired], settings, now, null);
    expect(result).toHaveLength(1);
    expect(result[0].chains[0].latest.id).toBe("third");
    expect(result[0].chains[0].previous.map((item) => item.id)).toEqual(["second", "first"]);
    expect(visibleJobIds(result, {}, {})).toEqual(["third", "second", "first"]);
    expect(visibleJobIds(result, { [result[0].key]: false }, {})).toEqual([]);
  });
  it("shows three previous jobs first and expands the rest", () => {
    const chain = Array.from({ length: 5 }, (_, index) => ({ ...job(`job-${index}`, "completed", `2026-10-08T0${index + 1}:00:00Z`),
      context: context("会話", `依頼 ${index}`), previousJobId: index ? `job-${index - 1}` : null }));
    const result = groups(chain, settings, now, null, ["job-4"]);
    expect(visibleJobIds(result, {}, {})).toEqual(["job-4", "job-3", "job-2", "job-1"]);
    expect(visibleJobIds(result, {}, { "job-4": true })).toEqual(["job-4", "job-3", "job-2", "job-1", "job-0"]);
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
