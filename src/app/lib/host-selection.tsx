import type { HostId, HostRecord } from "@core/generated/bindings";
import { createContext, useContext } from "react";

export interface HostSelection {
  hosts: readonly HostRecord[];
  selected: HostId;
  select: (host: HostId) => void;
}

export const HostSelectionContext = createContext<HostSelection | null>(null);
export const useHostSelection = () => useContext(HostSelectionContext);
