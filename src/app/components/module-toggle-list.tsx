import type { Module } from "@core/generated/bindings";
import { Switch } from "~/components/ui/switch";
import { cn } from "~/lib/utils";
import type { Accent } from "~/widgets/lib/accent";

export interface ModuleToggleItem {
  id: Module;
  label: string;
  /** What the module samples ("Load, clusters, per-core"). */
  description: string;
  accent: Accent;
  enabled: boolean;
  /** False when the host lacks the hardware; the row is shown disabled. */
  available: boolean;
}

export interface ModuleToggleListProps {
  modules: readonly ModuleToggleItem[];
  onToggle: (id: Module, enabled: boolean) => void;
}

/**
 * Module rows with swatch, name, description and a switch. A
 * module the host does not have stays listed, disabled, with
 * "Not present on this Mac" in place of its description.
 */
export function ModuleToggleList({ modules, onToggle }: ModuleToggleListProps) {
  return (
    <ul className="rounded-tile border border-border bg-well">
      {modules.map((m) => {
        const switchId = `module-toggle-${m.id}`;
        return (
          <li
            key={m.id}
            className="flex h-[42px] items-center gap-2.5 border-border-subtle border-b px-3 last:border-b-0"
          >
            <span
              aria-hidden
              className={cn(
                "size-2 shrink-0 rounded-mark",
                !m.available && "opacity-40"
              )}
              style={{ background: `var(--color-${m.accent})` }}
            />
            <div className="flex min-w-0 flex-1 flex-col">
              <label
                htmlFor={switchId}
                className={cn(
                  "text-[13px]",
                  !m.available && "text-muted-foreground"
                )}
              >
                {m.label}
              </label>
              <span className="truncate font-normal text-[11px] text-muted-foreground">
                {m.available ? m.description : "Not present on this Mac"}
              </span>
            </div>
            <Switch
              id={switchId}
              checked={m.available && m.enabled}
              disabled={!m.available}
              onCheckedChange={(on) => onToggle(m.id, on)}
            />
          </li>
        );
      })}
    </ul>
  );
}
