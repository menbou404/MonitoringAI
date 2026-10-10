import type { JobSummary } from "../bindings/JobSummary";
import { clock, dateMs, endMs, startMs } from "./format";
import { isActive } from "./jobList";

export function runningJobs(jobs: JobSummary[]): JobSummary[] {
  const rank = (job: JobSummary) => job.effectiveState === "orphaned" ? 0 : job.effectiveState === "quiet" ? 1 : 2;
  return jobs.filter(isActive).sort((a, b) => rank(a) - rank(b) || startMs(b) - startMs(a) || a.id.localeCompare(b.id));
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
