import { describe, expect, it } from "vitest";
import type { JobDetail } from "../bindings/JobDetail";
import type { JobSummary } from "../bindings/JobSummary";
import type { Settings } from "../bindings/Settings";
import type { UiSnapshot } from "../bindings/UiSnapshot";
import { elapsed, finishedTime, phaseName, requestLead, stateName } from "./format";
import { countSummary, groups, hiddenChains, moreLabel, shownChains, visibleJobIds } from "./jobList";
import { movementAgo, recentCompleted, runningJobs, runningWorkspaceName, runningWorkspaces, shortLatestMessage } from "./running";
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
    expect(visibleJobIds(result, {}, {})).toEqual(["third"]);
    expect(result[0].chains[0].previous).toHaveLength(2);
    expect(visibleJobIds(result, { [result[0].key]: false }, {})).toEqual([]);
  });
  it("keeps earlier jobs out of the list and counts the continuation", () => {
    const chain = Array.from({ length: 5 }, (_, index) => ({ ...job(`job-${index}`, "completed", `2026-10-08T0${index + 1}:00:00Z`),
      context: context("会話", `依頼 ${index}`), previousJobId: index ? `job-${index - 1}` : null }));
    const result = groups(chain, settings, now, null, ["job-4"]);
    expect(visibleJobIds(result, {}, {})).toEqual(["job-4"]);
    expect(result[0].chains[0].previous).toHaveLength(4);
  });
  it("summarizes counts, sorts by status, and folds completed and cancelled jobs after two", () => {
    const make = (id: string, state: JobSummary["effectiveState"], hour: number) => ({ ...job(id, state, `2026-10-08T${String(hour).padStart(2, "0")}:00:00Z`), context: context("会話", id),
      completedAt: ["completed", "failed", "cancelled"].includes(state) ? `2026-10-08T${String(hour).padStart(2, "0")}:00:00Z` : null });
    const jobs = [make("done1", "completed", 1), make("done2", "completed", 2), make("done3", "completed", 3), make("done4", "completed", 4),
      make("cancel", "cancelled", 5), make("failed", "failed", 6), make("running", "running", 7), make("unseen", "completed", 8)];
    const group = groups(jobs, settings, now, null, ["unseen"])[0];
    expect(countSummary(group)).toBe("実行中 1 · 完了 5 · 失敗 1 · キャンセル 1");
    expect(group.chains.map((chain) => chain.latest.id)).toEqual(["running", "unseen", "failed", "cancel", "done4", "done3", "done2", "done1"]);
    expect(shownChains(group, false, ["unseen"]).map((chain) => chain.latest.id)).toEqual(["running", "unseen", "failed", "cancel", "done4"]);
    expect(hiddenChains(group, ["unseen"])).toHaveLength(3);
    expect(moreLabel(hiddenChains(group, ["unseen"]))).toBe("完了 3件を表示");
    expect(moreLabel([{ latest: jobs[4], previous: [] }])).toBe("完了・キャンセル 1件を表示");
    expect(shownChains(group, true, ["unseen"])).toHaveLength(8);
    expect(visibleJobIds([group], {}, {}, ["unseen"])).toHaveLength(5);
  });
  it("keeps two simultaneous requests in one conversation distinct and groups unknown context by workspace", () => {
    const a = { ...job("a", "running"), context: context("same", "画面"), workspaceName: "Shared" };
    const b = { ...job("b", "running"), context: context("same", "テスト"), workspaceName: "Shared" };
    const unknown = { ...job("c", "completed"), workspaceName: "Other", workspaceRoot: "C:\\Other" };
    const result = groups([a, b, unknown], settings, now, null);
    expect(result[0].chains.map((chain) => requestLead(chain.latest))).toEqual(["画面", "テスト"]);
    expect(result[1].title).toBe("会話が不明 — Other");
    expect(groups([a, b, unknown], settings, now, "Shared")).toHaveLength(1);
  });
  it("keeps a conversation with seven completed requests short", () => {
    const jobs = Array.from({ length: 7 }, (_, index) => ({ ...job(`done-${index}`, "completed", `2026-10-08T0${index + 1}:00:00Z`),
      context: context("long", `依頼 ${index}`) }));
    const group = groups(jobs, settings, now, null, ["done-0"])[0];
    expect(visibleJobIds([group], {}, {}, ["done-0"])).toHaveLength(3);
    expect(hiddenChains(group, ["done-0"])).toHaveLength(4);
    expect(visibleJobIds([group], {}, { [group.key]: true }, ["done-0"])).toHaveLength(7);
  });
});
describe("running", () => {
  it("groups by full workspace root, counts active and attention, and orders folders and cards", () => {
    const a = { ...job("a", "running", "2026-10-08T11:00:00Z"), workspaceRoot: "C:\\Projects\\Same" };
    const b = { ...job("b", "quiet", "2026-10-08T09:00:00Z"), workspaceRoot: "D:\\Other\\Same" };
    const c = { ...job("c", "orphaned", "2026-10-08T08:00:00Z"), workspaceRoot: "D:\\Other\\Same" };
    const d = { ...job("d", "running", "2026-10-08T10:00:00Z"), workspaceRoot: null };
    const e = { ...job("e", "completed"), workspaceRoot: "D:\\Other\\Same" };
    const groups = runningWorkspaces([a, b, c, d, e]);
    expect(groups.map((group) => group.root)).toEqual(["D:\\Other\\Same", "C:\\Projects\\Same", null]);
    expect(groups.map((group) => group.name)).toEqual(["Same", "Same", "フォルダが不明"]);
    expect(groups[0].jobs.map((item) => item.id)).toEqual(["c", "b"]);
    expect([groups[0].activeCount, groups[0].attentionCount]).toEqual([2, 2]);
    expect([groups[1].activeCount, groups[1].attentionCount]).toEqual([1, 0]);
    expect(runningWorkspaceName("C:\\Projects\\Same\\")).toBe("Same");
  });
  it("sorts folders with the latest start first within the same attention level", () => {
    const old = { ...job("old", "quiet", "2026-10-08T08:00:00Z"), workspaceRoot: "C:\\Old" };
    const recent = { ...job("recent", "quiet", "2026-10-08T09:00:00Z"), workspaceRoot: "C:\\Recent" };
    expect(runningWorkspaces([old, recent]).map((group) => group.name)).toEqual(["Recent", "Old"]);
  });
  it("removes fixed command prefixes and preserves other messages", () => {
    expect(shortLatestMessage("Running command: cargo test --offline")).toBe("cargo test --offline");
    expect(shortLatestMessage("Command completed: cargo test (exit 0)")).toBe("cargo test (exit 0)");
    expect(shortLatestMessage("Command failed: cargo test (exit 1)")).toBe("cargo test (exit 1)");
    expect(shortLatestMessage("Applying 3 file changes.")).toBe("Applying 3 file changes.");
  });
  it("shows only active jobs, with orphaned then quiet then newest, and keeps three recent endings", () => {
    const jobs = [job("old", "running", "2026-10-08T08:00:00Z"), job("new", "running", "2026-10-08T10:00:00Z"),
      job("quiet", "quiet"), job("orphan", "orphaned"), ...[1, 2, 3, 4].map((hour) => job(`done${hour}`, "completed", `2026-10-08T0${hour}:00:00Z`))];
    expect(runningJobs(jobs).map((item) => item.id)).toEqual(["orphan", "quiet", "new", "old"]);
    expect(recentCompleted(jobs, now, 3).slice(0, 3).map((item) => item.id)).toEqual(["done4", "done3", "done2"]);
    expect(recentCompleted(jobs, now, 3).length - 1).toBe(3);
  });
  it("formats movement boundaries and omits missing timestamps", () => {
    expect(movementAgo(null, now)).toBeNull();
    expect(movementAgo("2026-10-08T11:59:01Z", now)).toBe("動き 59秒前");
    expect(movementAgo("2026-10-08T11:00:00Z", now)).toBe("動き 1時間前");
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
  const detail: JobDetail = { ...job("one", "running"), prompt: "Full request", turnId: null, model: null,
    effort: null, rendered: "Result", errorMessage: null, logFile: null };
  const log = { text: "Log output", start: 0, end: 10, truncatedBefore: false };
  it("starts on running and opens tray and card jobs on all", () => {
    expect(initialState.view).toBe("running");
    const focused = reducer(initialState, { type: "openJob", id: "one" });
    expect([focused.view, focused.selected]).toEqual(["all", "one"]);
    const running = reducer(focused, { type: "view", view: "running" });
    const card = reducer(running, { type: "openJob", id: "two" });
    expect([card.view, card.selected]).toEqual(["all", "two"]);
  });

  it("keeps detail and log when selecting the same job again", () => {
    let state = reducer(initialState, { type: "select", id: "one" });
    state = reducer(state, { type: "detail", value: detail });
    state = reducer(state, { type: "log", value: log });

    const selectedAgain = reducer(state, { type: "select", id: "one" });
    expect(selectedAgain).toBe(state);
    expect(selectedAgain.detail).toBe(detail);
    expect(selectedAgain.log).toBe(log);
  });

  it("clears detail and log when selecting a different job", () => {
    let state = reducer(initialState, { type: "select", id: "one" });
    state = reducer(state, { type: "detail", value: detail });
    state = reducer(state, { type: "log", value: log });

    const selectedOther = reducer(state, { type: "select", id: "two" });
    expect(selectedOther.selected).toBe("two");
    expect(selectedOther.detail).toBeNull();
    expect(selectedOther.log).toBeNull();
  });

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
