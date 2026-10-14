import { targetKey } from "@core/process-selection";
import { Button } from "~/components/ui/button";
import { quitExclusion } from "~/hooks/use-quit-flow";
import { useProcessSelection } from "./process-selection-context";
import {
  ProcessSelectionHead,
  ProcessSelectionRow,
} from "./process-selection-row";
/** Shared with virtual table viewport calculations. */
export const SELECTION_BAR_PX = 40;
export function SelectedProcessTable() {
  const ctx = useProcessSelection();
  if (!ctx) return null;
  const { selected } = ctx;
  return (
    <div data-process-scroll className="max-h-96 min-h-0 overflow-auto">
      <table aria-label="Selected processes" className="w-full text-[12px]">
        <thead>
          <tr>
            <ProcessSelectionHead />
            <th className="px-3 text-left font-normal">Process</th>
            <th className="px-3 text-right font-normal">PID</th>
            <th className="px-3 text-left font-normal">Last observed</th>
          </tr>
        </thead>
        <tbody>
          {selected.map((p) => (
            <ProcessSelectionRow
              key={targetKey(p)}
              rowKey={targetKey(p)}
              label={`${p.name} (${p.pid})`}
              targets={[p]}
            >
              <td className="px-3">{p.name}</td>
              <td className="figures px-3 text-right">{p.pid}</td>
              <td className="px-3 text-muted-foreground">
                {quitExclusion(p) ??
                  "Running when observed; checked again before sending"}
              </td>
            </ProcessSelectionRow>
          ))}
        </tbody>
      </table>
    </div>
  );
}
export function ProcessSelectionSummary({
  only,
  hidden,
  setOnly,
  clear,
}: {
  only: boolean;
  hidden: number;
  setOnly: (value: boolean) => void;
  clear: () => void;
}) {
  const ctx = useProcessSelection();
  if (!ctx) return null;
  const { selected, flow } = ctx;
  const eligible = selected.filter((p) => quitExclusion(p) === null).length;
  return (
    <>
      {" "}
      {selected.length > 0 && (
        <div
          className="flex shrink-0 flex-wrap items-center gap-2 border-border-subtle border-t px-3 py-1 text-[11px]"
          // Shared with virtual table viewport geometry.
          style={{ minHeight: SELECTION_BAR_PX }}
        >
          <span className="figures mr-auto whitespace-nowrap">
            {selected.length} selected
            {hidden > 0 && !only ? ` · ${hidden} hidden` : ""}
          </span>
          <Button
            variant="ghost"
            size="sm"
            aria-pressed={only}
            onClick={() => setOnly(!only)}
          >
            Selected only
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              clear();
              setOnly(false);
            }}
          >
            Clear
          </Button>
          <Button
            variant="secondary"
            size="sm"
            disabled={eligible === 0 || flow.busy}
            onClick={() => flow.requestMany(selected, "quit")}
          >
            Quit {selected.length}…
          </Button>
          <Button
            variant="ghost"
            size="sm"
            disabled={eligible === 0 || flow.busy}
            onClick={() => flow.requestMany(selected, "force_quit")}
          >
            Force Quit {selected.length}…
          </Button>
        </div>
      )}
      {flow.results.length > 1 && (
        <details className="shrink-0 border-border-subtle border-t px-3 py-2 text-[12px]">
          <summary className="cursor-pointer">
            Batch results:{" "}
            {flow.results.filter((r) => r.state === "accepted").length} of{" "}
            {flow.results.length} requests accepted
          </summary>
          <ul className="max-h-52 overflow-auto">
            {flow.results.map((r) => (
              <li key={targetKey(r.target)} className="py-1">
                {r.target.name}{" "}
                <span className="figures">({r.target.pid})</span>: {r.message}
              </li>
            ))}
          </ul>
        </details>
      )}
    </>
  );
}
