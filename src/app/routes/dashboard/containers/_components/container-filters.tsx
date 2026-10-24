import { DropdownMenu } from "radix-ui";
import { memo } from "react";
import { SearchField } from "~/components/search-field";
import { Button } from "~/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { lifecycleLabel } from "../_lib/presentation";

function ContainerFiltersView({
  query,
  state,
  grouped,
  columns,
  set,
}: {
  query: string;
  state: string;
  grouped: boolean;
  columns: string[];
  set: (key: string, value: string | null) => void;
}) {
  return (
    <>
      <SearchField
        value={query}
        onChange={(value) => set("q", value || null)}
        placeholder="Search containers…"
        label="Search containers by name, image or project"
      />
      <Select
        value={state}
        onValueChange={(value) => set("state", value === "all" ? null : value)}
      >
        <SelectTrigger aria-label="Container state" size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="all">All states</SelectItem>
          {Object.entries(lifecycleLabel).map(([value, label]) => (
            <SelectItem key={value} value={value}>
              {label}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <DropdownMenu.Root>
        <DropdownMenu.Trigger asChild>
          <Button variant="outline" size="sm">
            View
          </Button>
        </DropdownMenu.Trigger>
        <DropdownMenu.Portal>
          <DropdownMenu.Content className="z-50 min-w-40 rounded-control border border-border bg-popover p-2 text-[12px] shadow-md">
            <DropdownMenu.CheckboxItem
              checked={grouped}
              onCheckedChange={(value) =>
                set("group", value ? "grouped" : null)
              }
              className="cursor-default rounded-control px-2 py-2 focus:bg-selected"
            >
              {grouped ? "✓ " : ""}Group by project
            </DropdownMenu.CheckboxItem>
            {[
              ["io", "Block I/O"],
              ["image", "Image"],
              ["ports", "Ports"],
              ["restarts", "Restarts"],
              ["pids", "PIDs"],
            ].map(([value, label]) => (
              <DropdownMenu.CheckboxItem
                key={value}
                checked={columns.includes(value ?? "")}
                onCheckedChange={(checked) =>
                  set(
                    "columns",
                    (checked
                      ? [...columns, value]
                      : columns.filter((c) => c !== value)
                    ).join(",") || null
                  )
                }
                className="cursor-default rounded-control px-2 py-2 focus:bg-selected"
              >
                {columns.includes(value ?? "") ? "✓ " : ""}
                {label}
              </DropdownMenu.CheckboxItem>
            ))}
          </DropdownMenu.Content>
        </DropdownMenu.Portal>
      </DropdownMenu.Root>
    </>
  );
}

export const ContainerFilters = memo(
  ContainerFiltersView,
  (previous, next) =>
    previous.query === next.query &&
    previous.state === next.state &&
    previous.grouped === next.grouped &&
    previous.set === next.set &&
    previous.columns.length === next.columns.length &&
    previous.columns.every((column, index) => column === next.columns[index])
);
