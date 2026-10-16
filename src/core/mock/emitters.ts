import type {
  CapabilitiesChanged,
  EventRecorded,
  HistoryHealthChanged,
  HostsChanged,
  NavigateRequested,
  SettingsChanged,
  UpdateStatus,
  WindowAppearanceChanged,
} from "@core/generated/bindings";
import { createEmitter } from "./emitter";
export function createMockEmitters() {
  return {
    updates: createEmitter<UpdateStatus>(),
    settings: createEmitter<SettingsChanged>(),
    caps: createEmitter<CapabilitiesChanged>(),
    appearance: createEmitter<WindowAppearanceChanged>(),
    navigate: createEmitter<NavigateRequested>(),
    health: createEmitter<HistoryHealthChanged>(),
    hosts: createEmitter<HostsChanged>(),
    event: createEmitter<EventRecorded>(),
  };
}
export type MockEmitters = ReturnType<typeof createMockEmitters>;
