import type { JobSummary } from "../bindings/JobSummary";
import type { Settings } from "../bindings/Settings";
import { endMs, startMs, workspaceName } from "./format";

export const isActive = (job: JobSummary) => ["queued", "running", "quiet", "orphaned"].includes(job.effectiveState);
export function groups(jobs: JobSummary[], settings: Settings, now: number, workspace: string | null) {
  const filtered = jobs.filter((job) => workspace === null || workspaceName(job) === workspace);
  const active = filtered.filter(isActive).sort((a, b) => startMs(b) - startMs(a) || a.id.localeCompare(b.id));
  const recent = filtered.filter((job) => !isActive(job) && endMs(job) >= now - settings.recent_hours * 3_600_000)
    .sort((a, b) => endMs(b) - endMs(a) || a.id.localeCompare(b.id)).slice(0, settings.recent_limit);
  return { active, recent };
}
export function workspaces(jobs: JobSummary[]): string[] {
  return [...new Set(jobs.map(workspaceName))].sort((a, b) => a.localeCompare(b, "ja"));
}
