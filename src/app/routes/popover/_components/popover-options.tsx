import type { CardDensity } from "@core/generated/bindings";
import { SlidersHorizontal } from "lucide-react";
import { DropdownMenu } from "radix-ui";
import { Button } from "~/components/ui/button";
import { useTransport } from "~/lib/transport-context";

export function PopoverOptions({
  setAll,
}: {
  setAll: (density: CardDensity) => void;
}) {
  const transport = useTransport();
  const item =
    "cursor-default rounded-control px-2 py-1.5 text-[12px] outline-none data-highlighted:bg-muted";
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <Button variant="outline" size="icon" aria-label="Popover options">
          <SlidersHorizontal strokeWidth={1.5} />
        </Button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="end"
          sideOffset={6}
          className="z-50 min-w-36 rounded-card border bg-popover p-1 text-popover-foreground outline-none"
        >
          <DropdownMenu.Item
            className={item}
            onSelect={() => setAll("compact")}
          >
            Collapse all
          </DropdownMenu.Item>
          <DropdownMenu.Item
            className={item}
            onSelect={() => setAll("expanded")}
          >
            Expand all
          </DropdownMenu.Item>
          <DropdownMenu.Separator className="my-1 h-px bg-border" />
          <DropdownMenu.Item
            className={item}
            onSelect={() => void transport.openDashboard("/dashboard/settings")}
          >
            Open settings
          </DropdownMenu.Item>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}
