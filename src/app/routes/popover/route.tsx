import { windowMean } from "@core/series-stats";
import { useState } from "react";
import { PopoverFooter } from "~/components/popover-footer";
import { PopoverHeader } from "~/components/popover-header";
import { UnavailableCard } from "~/components/unavailable-card";
import { useHostRecord } from "~/hooks/use-host-record";
import { useModuleStates } from "~/hooks/use-module-states";
import { useNow } from "~/hooks/use-now";
import { useScaledWindow } from "~/hooks/use-scaled-window";
import { useTransport } from "~/lib/transport-context";
import { cn } from "~/lib/utils";
import { useHost } from "~/stores/host-store";
import { useSampling } from "~/stores/live-selectors";
import {
  LiveBatteryCard,
  LiveCoresCard,
  LiveCpuCard,
  LiveGpuCard,
  LiveMemoryCard,
  LiveNetworkCard,
  LivePowerCard,
} from "./_components/popover-cards";
import { PopoverScroll } from "./_components/popover-scroll";
import { type PopoverCard, popoverSlots } from "./_lib/card-order";
import { samplingPill } from "./_lib/sampling-pill";

const CARDS: Record<PopoverCard, () => React.JSX.Element> = {
  cpu: LiveCpuCard,
  cores: LiveCoresCard,
  memory: LiveMemoryCard,
  gpu: LiveGpuCard,
  power: LivePowerCard,
  network: LiveNetworkCard,
  battery: LiveBatteryCard,
};

/**
 * Footer with Kelvo's own CPU averaged over the last 60 s. Its own component
 * because it re-renders on every appended row; the cards must not.
 */
function LiveFooter() {
  const transport = useTransport();
  const windowMs = useScaledWindow(60_000);
  const selfCpu = useHost((s) => windowMean(s, "self.cpu", windowMs));
  return (
    <PopoverFooter
      selfCpuPct={selfCpu}
      onOpenDashboard={() => void transport.openDashboard(null)}
      onActivity={() => void transport.openDashboard("/dashboard/processes")}
    />
  );
}

/**
 * Popover panel (plan 4.3): fixed header and footer, and
 * the module cards in between in module order for enabled modules the host
 * has. The live store asks for a 60 s backfill on subscribe, so the charts
 * are full on the first frame. A stale stream dims the values to 50% until
 * the next frame.
 */
export default function PopoverRoute() {
  const transport = useTransport();
  const host = useHostRecord();
  const sampling = useSampling();
  const slots = popoverSlots(useModuleStates());
  const [scrolled, setScrolled] = useState(false);
  const now = useNow(60_000);
  const stale = sampling.stale && !sampling.paused;

  return (
    <div className="surface-vibrant flex h-svh flex-col overflow-hidden rounded-card text-foreground">
      <PopoverHeader
        hostName={host?.display_name ?? ""}
        uptimeMs={host ? now - host.info.boot_time_ms : 0}
        status={samplingPill(sampling)}
        paused={sampling.paused}
        performance={sampling.performance}
        scrolled={scrolled}
        onPause={() => void transport.setPaused(!sampling.paused)}
        onSettings={() => void transport.openDashboard("/dashboard/settings")}
      />
      <PopoverScroll
        onScrolledChange={setScrolled}
        className={cn(
          "transition-opacity duration-(--motion-crossfade)",
          stale && "opacity-50"
        )}
      >
        {slots.map((slot) => {
          if (slot.kind === "unavailable") {
            return (
              <UnavailableCard
                key={slot.module}
                module={slot.module}
                state={slot.state}
              />
            );
          }
          const Card = CARDS[slot.card];
          return <Card key={slot.card} />;
        })}
      </PopoverScroll>
      <LiveFooter />
    </div>
  );
}
