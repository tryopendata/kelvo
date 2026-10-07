import { useLocation } from "react-router";

const TITLES: Record<string, string> = {
  timeline: "Timeline",
  cpu: "CPU",
  gpu: "GPU",
  memory: "Memory",
  power: "Power & Sensors",
  network: "Network",
  disk: "Disk",
  battery: "Battery",
  processes: "Processes",
  settings: "Settings",
};

/** Dashboard pages that land in phase 5: the title and nothing else yet. */
export default function PendingPageRoute() {
  const page = useLocation().pathname.split("/")[2] ?? "";
  return (
    <h1 className="font-[590] text-[22px] tracking-[-0.022em]">
      {TITLES[page] ?? page}
    </h1>
  );
}
