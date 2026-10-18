import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { UsageBar } from "./UsageBar";

describe("usage header", () => {
  it("highlights the threshold and shows the current model and app ownership", () => {
    const html = renderToStaticMarkup(<UsageBar threshold={90} status={{ usage: {
      fiveHour: { usedPercent: 90, resetsAt: 100 }, week: { usedPercent: 18, resetsAt: 200 },
    }, currentModel: "gpt-6-luna", switchedByApp: true, error: null }} />);
    expect(html).toContain('class="usage-high">5時間 90%');
    expect(html).toContain('class="">週 18%');
    expect(html).toContain("既定: gpt-6-luna");
    expect(html).toContain("節約用に切り替え中");
  });

  it("shows missing data and retry errors without claiming a switch", () => {
    const html = renderToStaticMarkup(<UsageBar threshold={90} status={{ usage: null,
      currentModel: null, switchedByApp: false, error: "5分後にもう一度試します" }} />);
    expect(html).toContain("5時間 —");
    expect(html).toContain("週 —");
    expect(html).toContain('role="alert"');
    expect(html).not.toContain('class="usage-switched"');
  });
});
