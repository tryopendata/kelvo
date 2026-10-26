import "../../../../src/app/app.css";
import { createMockTransport } from "@core/mock-transport";
import { Profiler } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../../../../src/app/app";

declare global {
  interface Window {
    popoverProfile: { commits: number[]; messages: string[]; tick: () => void };
  }
}

const transport = createMockTransport({
  windowLabel: "popover",
  autoTick: false,
});
window.popoverProfile = {
  commits: [],
  messages: [],
  tick: () => transport.tick(),
};
const subscribe = transport.subscribeLive;
transport.subscribeLive = (host, onMsg, options) =>
  subscribe(
    host,
    (message) => {
      window.popoverProfile.messages.push(message.kind);
      onMsg(message);
    },
    options
  );
const container = document.getElementById("root");
if (!container) throw new Error("Missing fixture root");
document.documentElement.dataset.window = "popover";
createRoot(container).render(
  <Profiler
    id="popover"
    onRender={(_id, _phase, _actual, _base, _start, commit) =>
      window.popoverProfile.commits.push(commit)
    }
  >
    <App
      transport={transport}
      router={{ initialEntry: "/popover", theme: "dark", dev: true }}
    />
  </Profiler>
);
