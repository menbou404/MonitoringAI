import type { JobSummary } from "../bindings/JobSummary";
import type { Settings } from "../bindings/Settings";
import { conversationTitle, endMs, startMs, workspaceName } from "./format";

export const isActive = (job: JobSummary) => ["queued", "running", "quiet", "orphaned"].includes(job.effectiveState);
export type JobChain = { latest: JobSummary; previous: JobSummary[] };
export type Conversation = { key: string; title: string; chains: JobChain[]; activeCount: number; counts: Record<string, number>;
  count: number; defaultOpen: boolean };

export function conversationKey(job: JobSummary): string {
  return job.context ? `session:${job.context.sessionId}` : `unknown:${job.workspaceRoot ?? workspaceName(job)}`;
}
export const needsAttention = (job: JobSummary, unseen: string[]) => job.effectiveState === "failed" || unseen.includes(job.id);
export const isCompact = (job: JobSummary, unseen: string[]) => ["completed", "cancelled"].includes(job.effectiveState) && !needsAttention(job, unseen);

export function shownChains(conversation: Conversation, expanded: boolean, unseen: string[]): JobChain[] {
  if (expanded) return conversation.chains;
  let compactCount = 0;
  return conversation.chains.filter((chain) => {
    if (!isCompact(chain.latest, unseen)) return true;
    compactCount++;
    return compactCount <= 2;
  });
}
export function hiddenChains(conversation: Conversation, unseen: string[]): JobChain[] {
  const visible = new Set(shownChains(conversation, false, unseen).map((chain) => chain.latest.id));
  return conversation.chains.filter((chain) => !visible.has(chain.latest.id));
}
export function countSummary(conversation: Conversation): string {
  const labels: [string, string][] = [["active", "実行中"], ["completed", "完了"], ["failed", "失敗"], ["cancelled", "キャンセル"], ["unknown", "不明"]];
  return labels.filter(([key]) => conversation.counts[key]).map(([key, label]) => `${label} ${conversation.counts[key]}`).join(" · ");
}
export function moreLabel(chains: JobChain[]): string {
  return `${chains.some((chain) => chain.latest.effectiveState === "cancelled") ? "完了・キャンセル" : "完了"} ${chains.length}件を表示`;
}

export function groups(jobs: JobSummary[], settings: Settings, now: number, workspace: string | null, unseen: string[] = []): Conversation[] {
  const buckets = new Map<string, JobSummary[]>();
  for (const job of jobs) {
    if (workspace !== null && workspaceName(job) !== workspace) continue;
    const key = conversationKey(job);
    buckets.set(key, [...(buckets.get(key) ?? []), job]);
  }
  const conversations = [...buckets].flatMap(([key, members]): Conversation[] => {
    const byId = new Map(members.map((job) => [job.id, job]));
    const referenced = new Set(members.map((job) => job.previousJobId).filter((id): id is string => !!id));
    const visited = new Set<string>();
    const chains: JobChain[] = [];
    const heads = members.filter((job) => !referenced.has(job.id)).sort((a, b) => startMs(b) - startMs(a));
    for (const head of [...heads, ...members]) {
      if (visited.has(head.id)) continue;
      visited.add(head.id);
      const previous: JobSummary[] = [];
      let current = head;
      while (current.previousJobId) {
        const prior = byId.get(current.previousJobId);
        if (!prior || visited.has(prior.id)) break;
        previous.push(prior);
        visited.add(prior.id);
        current = prior;
      }
      chains.push({ latest: head, previous });
    }
    const current = chains.filter(({ latest }) => isActive(latest) || endMs(latest) >= now - settings.list_days * 86_400_000);
    if (!current.length) return [];
    current.sort((a, b) => {
      const rank = (job: JobSummary) => isActive(job) ? 0 : needsAttention(job, unseen) ? 1 : 2;
      return rank(a.latest) - rank(b.latest) || (isActive(a.latest) ? startMs(b.latest) - startMs(a.latest) : endMs(b.latest) - endMs(a.latest))
        || a.latest.id.localeCompare(b.latest.id);
    });
    const counts: Record<string, number> = {};
    for (const { latest } of current) {
      const category = isActive(latest) ? "active" : latest.effectiveState;
      counts[category] = (counts[category] ?? 0) + 1;
    }
    const representative = members.find((job) => job.context?.sessionTitle?.trim()) ?? members[0];
    return [{ key, title: key.startsWith("unknown:") ? `会話が不明 — ${workspaceName(representative)}` : conversationTitle(representative),
      chains: current, counts, activeCount: counts.active ?? 0, count: current.length,
      defaultOpen: current.some(({ latest, previous }) => isActive(latest) || [latest, ...previous].some((job) => unseen.includes(job.id))) }];
  });
  conversations.sort((a, b) => {
    if (!!a.activeCount !== !!b.activeCount) return b.activeCount ? 1 : -1;
    const activity = (group: Conversation) => group.activeCount
      ? Math.max(...group.chains.filter(({ latest }) => isActive(latest)).map(({ latest }) => startMs(latest)))
      : Math.max(...group.chains.map(({ latest }) => endMs(latest)));
    return activity(b) - activity(a) || a.key.localeCompare(b.key);
  });
  return conversations;
}

export function visibleJobIds(conversations: Conversation[], expanded: Record<string, boolean>, expandedCompleted: Record<string, boolean>, unseen: string[] = []): string[] {
  return conversations.flatMap((conversation) => (expanded[conversation.key] ?? conversation.defaultOpen)
    ? shownChains(conversation, !!expandedCompleted[conversation.key], unseen).map((chain) => chain.latest.id) : []);
}

export function workspaces(jobs: JobSummary[]): string[] {
  return [...new Set(jobs.map(workspaceName))].sort((a, b) => a.localeCompare(b, "ja"));
}
