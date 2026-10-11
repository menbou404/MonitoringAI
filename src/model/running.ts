import type { JobSummary } from "../bindings/JobSummary";
import { clock, dateMs, endMs, startMs } from "./format";
import { isActive } from "./jobList";

export function runningJobs(jobs: JobSummary[]): JobSummary[] {
  const rank = (job: JobSummary) => job.effectiveState === "orphaned" ? 0 : job.effectiveState === "quiet" ? 1 : 2;
  return jobs.filter(isActive).sort((a, b) => rank(a) - rank(b) || startMs(b) - startMs(a) || a.id.localeCompare(b.id));
}
export type RunningWorkspace = { key: string; root: string | null; name: string; jobs: JobSummary[]; activeCount: number; attentionCount: number };

export function runningWorkspaceName(root: string | null): string {
  if (!root) return "フォルダが不明";
  return root.split(/[\\/]/).filter(Boolean).at(-1) ?? "フォルダが不明";
}

export function runningFolderKey(job: JobSummary): string {
  return job.workspaceRoot || `unknown:${job.workspaceName?.trim() || "フォルダが不明"}`;
}

export function runningCardActivation(suppressClick: boolean, event: "dragstart" | "mousedown" | "click"):
  { suppressClick: boolean; open: boolean } {
  if (event === "dragstart") return { suppressClick: true, open: false };
  if (event === "mousedown") return { suppressClick: false, open: false };
  return { suppressClick: false, open: !suppressClick };
}

function ordered<T>(items: T[], saved: string[], key: (item: T) => string): T[] {
  const positions = new Map(saved.map((value, index) => [value, index]));
  const newItems = items.filter((item) => !positions.has(key(item)));
  const savedItems = items.filter((item) => positions.has(key(item)))
    .sort((a, b) => positions.get(key(a))! - positions.get(key(b))!);
  return [...newItems, ...savedItems];
}

export function runningWorkspaces(jobs: JobSummary[], folderOrder: string[] = [], cardOrder: Record<string, string[]> = {}): RunningWorkspace[] {
  const byRoot = new Map<string, JobSummary[]>();
  for (const job of jobs) {
    if (!isActive(job)) continue;
    const key = runningFolderKey(job);
    byRoot.set(key, [...(byRoot.get(key) ?? []), job]);
  }
  const workspaces = [...byRoot].map(([key, members]) => ({
    key, root: members[0].workspaceRoot || null, name: runningWorkspaceName(members[0].workspaceRoot),
    jobs: ordered(runningJobs(members), cardOrder[key] ?? [], (job) => job.id), activeCount: members.length,
    attentionCount: members.filter((job) => job.effectiveState === "orphaned" || job.effectiveState === "quiet").length,
  }));
  workspaces.sort((a, b) => Number(b.attentionCount > 0) - Number(a.attentionCount > 0)
    || Math.max(...b.jobs.map(startMs)) - Math.max(...a.jobs.map(startMs))
    || a.key.localeCompare(b.key));
  return ordered(workspaces, folderOrder, (workspace) => workspace.key);
}

function move(ids: string[], source: string, target: string, after: boolean): string[] | null {
  if (!ids.includes(source) || !ids.includes(target)) return null;
  if (source === target) return ids;
  const remaining = ids.filter((id) => id !== source);
  const index = remaining.indexOf(target) + Number(after);
  remaining.splice(index, 0, source);
  return remaining;
}

export function reorderFolders(workspaces: RunningWorkspace[], source: string, target: string, after: boolean): string[] | null {
  return move(workspaces.map((workspace) => workspace.key), source, target, after);
}

export function reorderCards(workspaces: RunningWorkspace[], sourceFolder: string, targetFolder: string, source: string, target: string, after: boolean): string[] | null {
  if (sourceFolder !== targetFolder) return null;
  const workspace = workspaces.find((entry) => entry.key === sourceFolder);
  return workspace ? move(workspace.jobs.map((job) => job.id), source, target, after) : null;
}

export function shortLatestMessage(message: string): string {
  return message.replace(/^(?:Running command:|Command completed:|Command failed:)\s*/, "");
}
export function recentCompleted(jobs: JobSummary[], now: number, days: number): JobSummary[] {
  return jobs.filter((job) => ["completed", "failed", "cancelled"].includes(job.effectiveState) && endMs(job) >= now - days * 86_400_000)
    .sort((a, b) => endMs(b) - endMs(a) || a.id.localeCompare(b.id));
}
export function movementAgo(logModifiedAt: string | null, now: number): string | null {
  if (!logModifiedAt) return null;
  const seconds = Math.floor(Math.max(0, now - dateMs(logModifiedAt)) / 1000);
  if (seconds < 60) return `動き ${seconds}秒前`;
  if (seconds < 3600) return `動き ${Math.floor(seconds / 60)}分前`;
  return `動き ${Math.floor(seconds / 3600)}時間前`;
}
export const completionTime = (job: JobSummary) => clock(job.completedAt ?? job.createdAt);
