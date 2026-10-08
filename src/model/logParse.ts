export type LogEntry = { key: number; time: string | null; title: string; body: string; kind: "line" | "command" | "failed" | "block" };
const header = /^\[([^\]]+)\] ?(.*)$/;
export function parseLog(text: string, start = 0): LogEntry[] {
  const lines = text.split("\n");
  const entries: LogEntry[] = [];
  let offset = start;
  for (const line of lines) {
    const content = line.replace(/\r$/, "");
    const match = content.match(header);
    if (match) {
      const title = match[2];
      const kind = title === "Assistant message" || title === "Final output" ? "block"
        : title.startsWith("Command failed:") ? "failed"
        : /^(Running command:|Command completed:)/.test(title) ? "command" : "line";
      entries.push({ key: offset, time: match[1], title, body: "", kind });
    } else if (entries.length && entries[entries.length - 1].kind === "block") {
      entries[entries.length - 1].body += (entries[entries.length - 1].body ? "\n" : "") + content;
    } else if (content) {
      entries.push({ key: offset, time: null, title: content, body: "", kind: "line" });
    }
    offset += new TextEncoder().encode(line + "\n").length;
  }
  return entries;
}
