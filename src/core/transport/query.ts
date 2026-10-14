import type { CommandError } from "@core/generated/bindings";
import type { CommandResult } from "./contracts";

/** Raised by query functions so TanStack Query sees a failed command. */
export class CommandFailure extends Error {
  readonly error: CommandError;
  constructor(error: CommandError) {
    super("message" in error ? `${error.kind}: ${error.message}` : error.kind);
    this.name = "CommandFailure";
    this.error = error;
  }
}

/** Unwrap a command result for a query function: data, or a thrown failure. */
export async function unwrap<T>(result: Promise<CommandResult<T>>): Promise<T> {
  const r = await result;
  if (r.status === "error") throw new CommandFailure(r.error);
  return r.data;
}
