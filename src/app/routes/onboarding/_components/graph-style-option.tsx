import {
  SPARK_SAMPLES,
  type TrayStyle,
  type TrayValues,
} from "~/components/tray-preview";
import { TrayStyleOption } from "~/components/tray-style-option";
import { useHost } from "~/stores/host-store";
import { useRecent } from "../_hooks/use-recent";

/**
 * The "Graph per module" card. It owns the tick subscription and
 * the sparkline's samples, so a tick re-renders this card, not the whole
 * setup step.
 */
export function GraphStyleOption({
  values,
  selected,
  onSelect,
}: {
  values: TrayValues;
  selected: boolean;
  onSelect: (style: TrayStyle) => void;
}) {
  const tick = useHost((s) => s.lastTsMs);
  const cpuHistory = useRecent(values.cpu, tick, SPARK_SAMPLES);
  return (
    <TrayStyleOption
      style="graphs"
      title="Graph per module"
      description="Separate items you can reorder with ⌘-drag."
      selected={selected}
      values={{ ...values, cpuHistory }}
      onSelect={onSelect}
    />
  );
}
