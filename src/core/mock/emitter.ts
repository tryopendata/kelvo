import type { Unsubscribe } from "../transport";

/** Listeners for one mock event, called in subscription order. */
export interface Emitter<T> {
  on(cb: (e: T) => void): Unsubscribe;
  /** Calls every listener subscribed when it starts. */
  emit(e: T): void;
  clear(): void;
}

export function createEmitter<T>(): Emitter<T> {
  const listeners = new Set<(e: T) => void>();
  return {
    on(cb) {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    emit(e) {
      for (const cb of [...listeners]) cb(e);
    },
    clear() {
      listeners.clear();
    },
  };
}
