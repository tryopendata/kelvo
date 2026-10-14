import {
  type ProcessTarget,
  targetKey,
  uniqueTargets,
} from "@core/process-selection";
import type { ComponentProps } from "react";
import { ProcessRowActions } from "~/components/process-actions";
import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "~/components/ui/context-menu";
import { quitExclusion } from "~/hooks/use-quit-flow";
import { cn } from "~/lib/utils";
import { useProcessSelection } from "./process-selection-context";

/** Selection is neutral; module accents stay reserved for measured data. */
const NEUTRAL_CHECKBOX =
  "data-[state=checked]:border-border-strong data-[state=checked]:bg-selected data-[state=checked]:text-foreground dark:data-[state=checked]:bg-selected";
export function ProcessSelectionHead() {
  const ctx = useProcessSelection();
  if (!ctx) return null;
  const targets = uniqueTargets(
    ctx.allTargets.filter((p) => p.running !== false)
  );
  const count = targets.filter((p) => ctx.keys.has(targetKey(p))).length;
  return (
    <th scope="col" className="w-8 px-2">
      <Checkbox
        aria-label="Select all matching processes"
        disabled={targets.length === 0}
        className={NEUTRAL_CHECKBOX}
        checked={
          count === 0
            ? false
            : count === targets.length
              ? true
              : "indeterminate"
        }
        onClick={() => ctx.selectAll(count > 0 && count === targets.length)}
      />
    </th>
  );
}

/** Shared row chrome for both the virtualized live table and grouped usage tables. */
export function ProcessSelectionRow({
  rowKey,
  label,
  targets,
  children,
  className,
  ...props
}: ComponentProps<"tr"> & {
  rowKey: string;
  label: string;
  targets: readonly ProcessTarget[];
}) {
  const ctx = useProcessSelection();
  if (!ctx)
    return (
      <tr className={className} {...props}>
        {children}
      </tr>
    );
  const available = targets.filter((p) => p.running !== false);
  const count = available.filter((p) => ctx.keys.has(targetKey(p))).length;
  const all = available.length > 0 && count === available.length;
  const some = count > 0;
  const chosen = some ? ctx.selected : available;
  const actions = chosen.filter((p) => quitExclusion(p) === null).length;
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <tr
          {...props}
          data-process-key={rowKey}
          data-state={some ? "selected" : undefined}
          tabIndex={ctx.focused === rowKey ? 0 : -1}
          aria-selected={some}
          className={cn(
            "group h-[29px] border-border-subtle border-b hover:bg-selected focus-visible:outline-2 focus-visible:outline-ring focus-visible:-outline-offset-2 data-[state=selected]:bg-selected",
            className
          )}
          onFocus={(e) => {
            if (e.target === e.currentTarget) ctx.recordFocus(rowKey);
          }}
          onClick={(e) => {
            if (
              e.ctrlKey ||
              (e.target instanceof HTMLElement &&
                e.target.closest(
                  "button,input,[role=checkbox],a,[data-selection-gutter]"
                ))
            )
              return;
            ctx.select(rowKey, {
              range: e.shiftKey,
              additive: e.metaKey,
              toggle: e.metaKey,
            });
          }}
          onKeyDown={(e) => {
            props.onKeyDown?.(e);
            if (!e.defaultPrevented) ctx.keyDown(e, rowKey);
          }}
          onContextMenu={() => {
            if (!some && available.length) ctx.select(rowKey);
          }}
          onPointerEnter={() => ctx.dragOver(rowKey)}
        >
          <td
            className="w-8 select-none px-2"
            data-selection-gutter
            aria-label="Selection"
            onPointerDown={(e) => {
              if (
                available.length === 0 ||
                e.button !== 0 ||
                e.ctrlKey ||
                (e.target instanceof HTMLElement && e.target.closest("button"))
              )
                return;
              e.preventDefault();
              ctx.dragStart(rowKey, e.metaKey);
            }}
          >
            <Checkbox
              disabled={!available.length}
              aria-label={`Select ${label}`}
              className={NEUTRAL_CHECKBOX}
              checked={all ? true : some ? "indeterminate" : false}
              onClick={(e) => {
                e.stopPropagation();
                ctx.select(rowKey, {
                  range: e.shiftKey,
                  additive: true,
                  toggle: true,
                });
              }}
            />
          </td>
          {children}
        </tr>
      </ContextMenuTrigger>
      <ContextMenuContent className="w-60">
        <ContextMenuLabel>
          {chosen.length > 1 ? `${chosen.length} selected processes` : label}
        </ContextMenuLabel>
        <ContextMenuSeparator />
        <ContextMenuItem
          disabled={actions === 0}
          onSelect={() => ctx.flow.requestMany(chosen, "quit")}
        >
          {chosen.length > 1 ? `Quit ${chosen.length} processes…` : "Quit…"}
        </ContextMenuItem>
        <ContextMenuItem
          variant="destructive"
          disabled={actions === 0}
          onSelect={() => ctx.flow.requestMany(chosen, "force_quit")}
        >
          {chosen.length > 1
            ? `Force Quit ${chosen.length} processes…`
            : "Force Quit…"}
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}

export function SharedProcessActions({
  targets,
}: {
  targets: readonly ProcessTarget[];
}) {
  const ctx = useProcessSelection();
  if (!ctx || ctx.selected.length > 0) return null;
  const first = targets[0];
  if (targets.length === 1 && first)
    return <ProcessRowActions p={first} onRequest={ctx.flow.request} />;
  if (!targets.length) return null;
  return (
    <span className="inline-flex gap-1">
      <Button
        variant="ghost"
        size="sm"
        onClick={(e) => {
          e.stopPropagation();
          ctx.flow.requestMany(targets, "quit");
        }}
      >
        Quit {targets.length}…
      </Button>
      <Button
        variant="ghost"
        size="sm"
        onClick={(e) => {
          e.stopPropagation();
          ctx.flow.requestMany(targets, "force_quit");
        }}
      >
        Force Quit {targets.length}…
      </Button>
    </span>
  );
}
