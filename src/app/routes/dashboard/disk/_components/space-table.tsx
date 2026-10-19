import { formatBytes } from "@core/format";
import type {
  SpaceFile,
  SpaceNode,
  SpaceTarget,
} from "@core/generated/bindings";
import { spaceKey } from "@core/space-treemap";
import { Copy, File, Folder, FolderSearch } from "lucide-react";
import type { KeyboardEvent, ReactNode } from "react";
import { memo } from "react";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "~/components/ui/context-menu";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "~/components/ui/table";
import { cn } from "~/lib/utils";
import { MeterTrack } from "~/widgets/meter-track";
import {
  canOpen,
  itemsLabel,
  kindLabel,
  middleTruncate,
  shareLabel,
} from "../_lib/space";

export interface SpaceItemActions {
  reveal: (target: SpaceTarget) => void;
  copyPath: (target: SpaceTarget) => void;
}

/** Arrow keys move between rows; the rest is the caller's (open, up). */
function moveFocus(e: KeyboardEvent<HTMLElement>) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return false;
  if (e.metaKey) return false;
  const rows = [
    ...e.currentTarget.querySelectorAll<HTMLElement>("tr[data-row]"),
  ];
  const at = rows.indexOf(document.activeElement as HTMLElement);
  const next = rows[e.key === "ArrowDown" ? at + 1 : Math.max(0, at - 1)];
  if (next) {
    e.preventDefault();
    next.focus();
  }
  return true;
}

/**
 * The current folder's entries, linked to the treemap by `hovered`. The
 * keyboard path: arrows move, Enter or Cmd+↓ opens a folder (Finder's keys).
 */
export const SpaceTable = memo(function SpaceTable({
  rows,
  total,
  hovered,
  onHover,
  onOpen,
  onUp,
  actions,
}: {
  rows: readonly SpaceNode[];
  total: number;
  hovered: string | null;
  onHover: (key: string | null) => void;
  onOpen: (node: SpaceNode) => void;
  /** Cmd+↑ or Escape: the parent folder. */
  onUp: () => void;
  actions: SpaceItemActions;
}) {
  return (
    <Table aria-label="Folder contents">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead>Name</TableHead>
          <TableHead className="w-24 text-right">Size</TableHead>
          <TableHead className="w-40">Share</TableHead>
          <TableHead className="w-28 text-right">Items</TableHead>
          <TableHead className="w-16">
            <span className="sr-only">Actions</span>
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody
        onKeyDown={(e) => {
          // Row menus render in a portal: their keys are theirs.
          if (!e.currentTarget.contains(e.target as Node)) return;
          if ((e.key === "ArrowUp" && e.metaKey) || e.key === "Escape") {
            e.preventDefault();
            onUp();
            return;
          }
          if (moveFocus(e)) return;
          const key = (document.activeElement as HTMLElement | null)?.dataset
            .row;
          const node = rows.find((r) => spaceKey(r) === key) ?? null;
          const open =
            e.key === "Enter" || (e.key === "ArrowDown" && e.metaKey);
          if (open && canOpen(node)) {
            e.preventDefault();
            onOpen(node);
          }
        }}
        onPointerLeave={() => onHover(null)}
      >
        {rows.map((r) => (
          <NodeRow
            key={spaceKey(r)}
            node={r}
            total={total}
            active={hovered === spaceKey(r)}
            onHover={onHover}
            onOpen={onOpen}
            actions={actions}
          />
        ))}
      </TableBody>
    </Table>
  );
});

const NodeRow = memo(function NodeRow({
  node,
  total,
  active,
  onHover,
  onOpen,
  actions,
}: {
  node: SpaceNode;
  total: number;
  active: boolean;
  onHover: (key: string | null) => void;
  onOpen: (node: SpaceNode) => void;
  actions: SpaceItemActions;
}) {
  const key = spaceKey(node);
  const pending = node.kind === "folder" && !node.done;
  const share = total > 0 ? (node.bytes ?? 0) / total : 0;
  const opens = canOpen(node);
  const target: SpaceTarget | null =
    node.id === null ? null : { kind: "node", id: node.id };
  const row = (
    <TableRow
      data-row={key}
      tabIndex={0}
      aria-label={`${node.name}, ${pending ? "scanning" : formatBytes(node.bytes)}${opens ? ", folder" : ""}`}
      className={cn(
        "group text-[12px] outline-none focus-visible:bg-selected focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
        active && "bg-selected",
        opens && "cursor-pointer"
      )}
      onPointerEnter={() => onHover(key)}
      onFocus={() => onHover(key)}
      onClick={() => {
        if (opens) onOpen(node);
      }}
    >
      <NameCell node={node} />
      <TableCell
        className={cn("figures text-right", pending && "text-muted-foreground")}
      >
        {pending ? "Scanning…" : formatBytes(node.bytes)}
      </TableCell>
      <TableCell>{!pending && <ShareCell share={share} />}</TableCell>
      <TableCell className="figures text-right text-fg-subtle">
        {COUNTED.has(node.kind) ? itemsLabel(node.items) : ""}
      </TableCell>
      <TableCell>
        {target && (
          <RowActions name={node.name} target={target} actions={actions} />
        )}
      </TableCell>
    </TableRow>
  );
  return target ? (
    <ItemMenu name={node.name} target={target} actions={actions}>
      {row}
    </ItemMenu>
  ) : (
    row
  );
});

const COUNTED = new Set(["folder", "smaller"]);

function NameCell({ node }: { node: SpaceNode }) {
  const real = node.kind === "folder" || node.kind === "file";
  const note =
    node.kind === "other"
      ? "Snapshots, APFS metadata, unreadable and skipped folders"
      : real
        ? null
        : kindLabel(node.kind, node);
  return (
    <TableCell className="max-w-0">
      <div className="flex min-w-0 items-center gap-2">
        <KindIcon node={node} />
        <span
          className={cn(
            "shrink-0 truncate",
            real ? "text-foreground" : "text-muted-foreground"
          )}
          title={real ? node.name : (note ?? undefined)}
        >
          {node.kind === "other" ? "Other" : node.name}
        </span>
        {note && <span className="truncate text-fg-faint">{note}</span>}
      </div>
    </TableCell>
  );
}

function ShareCell({ share }: { share: number }) {
  return (
    <div className="flex items-center gap-2">
      <MeterTrack fraction={share} fill="var(--color-disk)" className="w-20" />
      <span className="figures w-12 text-fg-subtle">{shareLabel(share)}</span>
    </div>
  );
}

function KindIcon({ node }: { node: SpaceNode }) {
  const Icon = node.kind === "file" ? File : Folder;
  if (node.kind !== "folder" && node.kind !== "file")
    return <span aria-hidden className="size-3.5 shrink-0" />;
  return (
    <Icon
      aria-hidden
      className="size-3.5 shrink-0 text-(--color-disk-ink)"
      strokeWidth={1.75}
    />
  );
}

function RowActions({
  name,
  target,
  actions,
}: {
  name: string;
  target: SpaceTarget;
  actions: SpaceItemActions;
}) {
  const button =
    "grid size-6 place-items-center rounded-control text-fg-subtle opacity-0 outline-none hover:bg-selected hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring group-hover:opacity-100 group-focus-within:opacity-100";
  return (
    <div className="flex justify-end gap-0.5">
      <button
        type="button"
        className={button}
        aria-label={`Show ${name} in Finder`}
        title="Show in Finder"
        onClick={(e) => {
          e.stopPropagation();
          actions.reveal(target);
        }}
      >
        <FolderSearch aria-hidden className="size-3.5" />
      </button>
      <button
        type="button"
        className={button}
        aria-label={`Copy the path of ${name}`}
        title="Copy path"
        onClick={(e) => {
          e.stopPropagation();
          actions.copyPath(target);
        }}
      >
        <Copy aria-hidden className="size-3.5" />
      </button>
    </div>
  );
}

function ItemMenu({
  name,
  target,
  actions,
  children,
}: {
  name: string;
  target: SpaceTarget;
  actions: SpaceItemActions;
  children: ReactNode;
}) {
  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent className="w-64">
        <ContextMenuLabel className="truncate">{name}</ContextMenuLabel>
        <ContextMenuSeparator />
        <ContextMenuItem onSelect={() => actions.reveal(target)}>
          Show in Finder
        </ContextMenuItem>
        <ContextMenuItem onSelect={() => actions.copyPath(target)}>
          Copy path
        </ContextMenuItem>
        <ContextMenuSeparator />
        <p className="px-2 py-1.5 text-[11px] text-muted-foreground">
          Deleting may not free space at once while local snapshots or clones
          still reference the data.
        </p>
      </ContextMenuContent>
    </ContextMenu>
  );
}

/** The 50 largest files anywhere in the scanned tree. */
export const LargestFilesTable = memo(function LargestFilesTable({
  files,
  actions,
}: {
  files: readonly SpaceFile[];
  actions: SpaceItemActions;
}) {
  return (
    <Table aria-label="Largest files">
      <TableHeader>
        <TableRow className="hover:bg-transparent">
          <TableHead>Name</TableHead>
          <TableHead>Folder</TableHead>
          <TableHead className="w-24 text-right">Size</TableHead>
          <TableHead className="w-16">
            <span className="sr-only">Actions</span>
          </TableHead>
        </TableRow>
      </TableHeader>
      <TableBody onKeyDown={moveFocus}>
        {files.map((f) => {
          const target: SpaceTarget = { kind: "largest_file", index: f.index };
          return (
            <ItemMenu
              key={f.index}
              name={f.name}
              target={target}
              actions={actions}
            >
              <TableRow
                data-row={f.index}
                tabIndex={0}
                className="group text-[12px] outline-none focus-visible:bg-selected focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
              >
                <TableCell className="max-w-0">
                  <div className="flex min-w-0 items-center gap-2">
                    <File
                      aria-hidden
                      className="size-3.5 shrink-0 text-(--color-disk-ink)"
                      strokeWidth={1.75}
                    />
                    <span className="truncate text-foreground" title={f.name}>
                      {f.name}
                    </span>
                  </div>
                </TableCell>
                <TableCell
                  className="max-w-0 truncate text-muted-foreground"
                  title={f.parent}
                >
                  {middleTruncate(f.parent, 64)}
                </TableCell>
                <TableCell className="figures text-right">
                  {formatBytes(f.bytes)}
                </TableCell>
                <TableCell>
                  <RowActions name={f.name} target={target} actions={actions} />
                </TableCell>
              </TableRow>
            </ItemMenu>
          );
        })}
      </TableBody>
    </Table>
  );
});
