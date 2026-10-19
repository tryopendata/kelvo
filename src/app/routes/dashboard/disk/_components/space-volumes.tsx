import type { SpaceVolume } from "@core/generated/bindings";
import { ChevronRight } from "lucide-react";
import { SectionCard } from "~/components/section-card";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import { VolumeTable } from "~/components/volume-table";
import { useUnits } from "~/hooks/use-units";

/**
 * The Space view before a report (plan "First impression"): the volumes this
 * Mac can scan, each with Scan, and the protected locations a scan skips
 * unless the user includes them.
 */
export function SpaceVolumes({
  volumes,
  include,
  onInclude,
  onScan,
  onOpen,
  reportVolume,
  scanningVolume,
  busy,
}: {
  volumes: readonly SpaceVolume[];
  include: ReadonlySet<string>;
  onInclude: (key: string, on: boolean) => void;
  onScan: (mount: string) => void;
  onOpen: (mount: string) => void;
  /** The volume the kept report is for, which opens instead of rescanning. */
  reportVolume: string | null;
  /** The volume being scanned now, whose row opens the running scan. */
  scanningVolume: string | null;
  /** A scan is running or starting. */
  busy: boolean;
}) {
  const units = useUnits().bytes;
  const protectedOnes = volumes.flatMap((v) => v.protected);
  return (
    <SectionCard
      accent="disk"
      title="Volumes"
      variant="default"
      className="overflow-hidden"
    >
      <p className="max-w-2xl text-[12px] text-muted-foreground">
        A space report measures every file on a volume and shows what uses the
        space. Kelvo reads file sizes only and changes nothing. Folders macOS
        protects are skipped unless you include them.
      </p>
      <section
        aria-label="Volumes to scan"
        // biome-ignore lint/a11y/noNoninteractiveTabindex: the horizontally scrollable volume table needs keyboard access
        tabIndex={0}
        className="min-w-0 max-w-full overflow-x-auto rounded-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <VolumeTable
          units={units}
          rows={volumes.map((v) => ({
            id: v.mount,
            name: v.name,
            usedBytes: v.used_bytes,
            freeBytes:
              v.total_bytes !== null && v.used_bytes !== null
                ? Math.max(0, v.total_bytes - v.used_bytes)
                : null,
            totalBytes: v.total_bytes,
          }))}
          action={(row) => (
            <div className="flex justify-end gap-1.5">
              {reportVolume === row.id && scanningVolume !== row.id && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => onOpen(row.id)}
                >
                  Open report
                </Button>
              )}
              {scanningVolume === row.id ? (
                <Button
                  size="sm"
                  variant="outline"
                  aria-label={`Show the scan of ${row.name}`}
                  onClick={() => onOpen(row.id)}
                >
                  Show scan
                </Button>
              ) : (
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy}
                  aria-label={`Scan ${row.name}`}
                  onClick={() => onScan(row.id)}
                >
                  {reportVolume === row.id ? "Scan again" : "Scan"}
                </Button>
              )}
            </div>
          )}
        />
      </section>
      {protectedOnes.length > 0 && (
        <ProtectedLocations
          locations={protectedOnes}
          include={include}
          onInclude={onInclude}
        />
      )}
    </SectionCard>
  );
}

function ProtectedLocations({
  locations,
  include,
  onInclude,
}: {
  locations: SpaceVolume["protected"];
  include: ReadonlySet<string>;
  onInclude: (key: string, on: boolean) => void;
}) {
  const skipped = locations.filter((l) => !include.has(l.key)).length;
  return (
    <details className="group text-[12px]">
      <summary className="flex w-fit cursor-default list-none items-center gap-1 rounded-sm text-fg-subtle outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden">
        <ChevronRight
          aria-hidden
          className="size-3.5 transition-transform duration-(--motion-fast) group-open:rotate-90"
        />
        Protected locations · {skipped} of {locations.length} skipped
      </summary>
      <div className="mt-2 flex flex-col gap-2 pl-4.5">
        <p className="max-w-2xl text-muted-foreground">
          Including one may show a macOS privacy prompt for Kelvo, and its size
          is otherwise part of Other.
        </p>
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-x-4 gap-y-1.5">
          {locations.map((l) => (
            <li key={l.key}>
              <div className="flex items-center gap-2">
                <Checkbox
                  id={`space-include-${l.key}`}
                  checked={include.has(l.key)}
                  onCheckedChange={(on) => onInclude(l.key, on === true)}
                />
                <label htmlFor={`space-include-${l.key}`}>{l.label}</label>
              </div>
            </li>
          ))}
        </ul>
      </div>
    </details>
  );
}
