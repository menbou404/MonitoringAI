import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { LogChunk } from "../bindings/LogChunk";
import { parseLog } from "../model/logParse";

export function LogView({ log, onOlder }: { log: LogChunk | null; onOlder: () => void }) {
  const parent = useRef<HTMLDivElement>(null);
  const previousEnd = useRef(0);
  const [following, setFollowing] = useState(true);
  const [newLog, setNewLog] = useState(false);
  const entries = useMemo(() => parseLog(log?.text ?? "", log?.start ?? 0), [log?.text, log?.start]);
  const virtual = useVirtualizer({ count: entries.length + (log?.truncatedBefore ? 1 : 0), getScrollElement: () => parent.current,
    estimateSize: () => 46, measureElement: (element) => element.getBoundingClientRect().height, overscan: 8 });
  useEffect(() => {
    const last = Math.max(0, entries.length - 1 + (log?.truncatedBefore ? 1 : 0));
    if (following) { virtual.scrollToIndex(last, { align: "end" }); setNewLog(false); }
    else if ((log?.end ?? 0) > previousEnd.current) setNewLog(true);
    previousEnd.current = log?.end ?? 0;
  }, [entries.length, log?.end, following]);
  const jump = () => { setFollowing(true); setNewLog(false); virtual.scrollToIndex(Math.max(0, entries.length - 1 + (log?.truncatedBefore ? 1 : 0)), { align: "end" }); };
  return <section className="log-section"><div className="section-heading">ログ</div>
    <div className="log-scroll" ref={parent} onScroll={(event) => {
      const el = event.currentTarget;
      const nearEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
      setFollowing(nearEnd); if (nearEnd) setNewLog(false);
    }}>
      <div style={{ height: virtual.getTotalSize(), position: "relative" }}>
        {virtual.getVirtualItems().map((item) => {
          const older = !!log?.truncatedBefore;
          const entry = entries[item.index - (older ? 1 : 0)];
          return <div key={item.key} ref={virtual.measureElement} data-index={item.index} style={{ position: "absolute", top: item.start, width: "100%" }}>
            {older && item.index === 0 ? <button className="older" onClick={onOlder}>さらに読み込む</button> : entry &&
              <div className={`log-entry ${entry.kind}`}><time>{entry.time ? new Date(entry.time).toLocaleTimeString("ja-JP", { hour12: false }) : ""}</time>
                <div><div className="log-title">{entry.title}</div>{entry.kind === "block" && <div className="log-body">{entry.body}</div>}</div></div>}
          </div>;
        })}
      </div>
    </div>
    {newLog && <button className="new-log" onClick={jump}>新しいログ ↓</button>}
  </section>;
}
