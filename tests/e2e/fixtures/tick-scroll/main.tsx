import "../../../../src/app/app.css";
import { useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { useTickScroll } from "../../../../src/app/widgets/lib/use-tick-scroll";
import { MirrorBars } from "../../../../src/app/widgets/mirror-bars";
import { StreamArea } from "../../../../src/app/widgets/stream-area";

declare global {
  interface Window {
    tickScroll: {
      update: (time: number, interval?: number) => void;
      unmount: () => void;
      reads: number;
    };
  }
}

const container = document.getElementById("root");
if (!container) throw new Error("Missing fixture root");
const root = createRoot(container);
window.tickScroll = {
  update: () => {},
  unmount: () => root.unmount(),
  reads: 0,
};
const bounds = Element.prototype.getBoundingClientRect;
Element.prototype.getBoundingClientRect = function () {
  window.tickScroll.reads++;
  return bounds.call(this);
};

function Fixture() {
  const [{ time, interval }, setTick] = useState({
    time: 1000,
    interval: 1000,
  });
  window.tickScroll.update = (time, interval = 1000) =>
    flushSync(() => setTick({ time, interval }));
  const html = useTickScroll<HTMLDivElement>(time, interval, "10%");
  const svg = useTickScroll<SVGGElement>(time, interval, "10px");
  if (new URLSearchParams(location.search).has("bars")) {
    return (
      <div style={{ width: 200 }}>
        <MirrorBars
          up={[1, 2, 3]}
          down={[3, 2, 1]}
          intervalMs={interval}
          tEndMs={time}
          accent="net"
          ariaLabel="Network test chart"
        />
      </div>
    );
  }
  if (new URLSearchParams(location.search).has("chart")) {
    return (
      <div data-chart style={{ width: 200 }}>
        <StreamArea
          series={[{ key: "cpu", values: [20, null, 60, 40, 20], step: 1 }]}
          tEndMs={time}
          intervalMs={interval}
          yMax={100}
          accent="cpu"
          height={40}
          ariaLabel="CPU test chart"
          gridlines={[50]}
          ceilingLabel="100%"
          windowLabel="4s"
        />
      </div>
    );
  }
  return (
    <>
      <div
        ref={html}
        data-scroll="html"
        style={{ width: 100, height: 20, background: "teal" }}
      />
      <svg width="100" height="20" role="img" aria-label="Scrolling SVG">
        <g ref={svg} data-scroll="svg">
          <rect width="100" height="20" fill="purple" />
        </g>
      </svg>
    </>
  );
}

flushSync(() => root.render(<Fixture />));
