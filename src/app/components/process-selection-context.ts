import type {
  ProcessTarget,
  SelectableProcessRow,
} from "@core/process-selection";
import { createContext, type KeyboardEvent, useContext } from "react";
import type { QuitFlow } from "~/hooks/use-quit-flow";
export interface SelectionContext {
  rows: readonly SelectableProcessRow[];
  selected: readonly ProcessTarget[];
  allTargets: readonly ProcessTarget[];
  keys: ReadonlySet<string>;
  focused: string | null;
  held: boolean;
  flow: QuitFlow;
  selectAll: (clear?: boolean) => void;
  select: (
    key: string,
    modifiers?: { range?: boolean; additive?: boolean; toggle?: boolean }
  ) => void;
  keyDown: (event: KeyboardEvent, key: string) => void;
  /** Record native focus without moving the row during a pointer gesture. */
  recordFocus: (key: string) => void;
  /** Reveal and focus a row for explicit keyboard navigation. */
  focus: (key: string) => void;
  reveal: { current: ((key: string) => void) | null };
  dragStart: (key: string, additive: boolean) => void;
  dragOver: (key: string) => void;
  dragging: { current: boolean };
}
export const Selection = createContext<SelectionContext | null>(null);
export const useProcessSelection = () => useContext(Selection);
