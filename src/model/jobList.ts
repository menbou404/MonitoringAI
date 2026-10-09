import type { JobSummary } from "../bindings/JobSummary";
import type { Settings } from "../bindings/Settings";
import { conversationTitle, endMs, startMs, workspaceName } from "./format";

export const isActive = (job: JobSummary) => ["queued", "running", "quiet", "orphaned"].includes(job.effectiveState);
export type JobChain = { latest: JobSummary; previous: JobSummary[] };
export type Conversation = { key: string; title: string; chains: JobChain[]; activeCount: number; count: number; defaultOpen: boolean };

export function conversationKey(job: JobSummary): string {
  return job.context ? `session:${job.context.sessionId}` : `unknown:${job.workspaceRoot ?? workspaceName(job)}`;
}

export function groups(jobs: JobSummary[], settings: Settings, now: number, workspace: string | null, unseen: string[] = []): Conversation[] {
  const eligible = jobs.filter((job) => (workspace === null || workspaceName(job) === workspace)
    && (isActive(job) || endMs(job) >= now - settings.list_days * 86_400_000));
  const buckets = new Map<string, JobSummary[]>();
  for (const job of eligible) {
    const key = conversationKey(job);
    buckets.set(key, [...(buckets.get(key) ?? []), job]);
  }
  const conversations = [...buckets].map(([key, members]): Conversation => {
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
    chains.sort((a, b) => Number(isActive(b.latest)) - Number(isActive(a.latest))
      || (isActive(a.latest) ? startMs(b.latest) - startMs(a.latest) : endMs(b.latest) - endMs(a.latest))
      || a.latest.id.localeCompare(b.latest.id));
    const activeCount = members.filter(isActive).length;
    const representative = members.find((job) => job.context?.sessionTitle?.trim()) ?? members[0];
    return { key, title: key.startsWith("unknown:") ? `会話が不明 — ${workspaceName(representative)}` : conversationTitle(representative),
      chains, activeCount, count: members.length, defaultOpen: activeCount > 0 || members.some((job) => unseen.includes(job.id)) };
  });
  conversations.sort((a, b) => {
    const aMembers = a.chains.flatMap((chain) => [chain.latest, ...chain.previous]);
    const bMembers = b.chains.flatMap((chain) => [chain.latest, ...chain.previous]);
    const aActive = aMembers.filter(isActive);
    const bActive = bMembers.filter(isActive);
    if (aActive.length || bActive.length) return Number(!!bActive.length) - Number(!!aActive.length)
      || Math.max(...bActive.map(startMs)) - Math.max(...aActive.map(startMs));
    return Math.max(...bMembers.map(endMs)) - Math.max(...aMembers.map(endMs)) || a.key.localeCompare(b.key);
  });
  return conversations;
}

export function visibleJobIds(conversations: Conversation[], expanded: Record<string, boolean>, expandedChains: Record<string, boolean>): string[] {
  return conversations.flatMap((conversation) => {
    if (!(expanded[conversation.key] ?? conversation.defaultOpen)) return [];
    return conversation.chains.flatMap((chain) => [chain.latest.id,
      ...chain.previous.slice(0, expandedChains[chain.latest.id] ? undefined : 3).map((job) => job.id)]);
  });
}

export function workspaces(jobs: JobSummary[]): string[] {
  return [...new Set(jobs.map(workspaceName))].sort((a, b) => a.localeCompare(b, "ja"));
}
