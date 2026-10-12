import type { CommandError } from "@core/generated/bindings";

export function remoteErrorText(error: CommandError): string {
  if ("message" in error) return error.message;
  switch (error.kind) {
    case "unknown_host":
      return "This Mac is no longer in your host list.";
    case "remote_host":
      return "This Mac does not share this data.";
    case "history_unavailable":
      return "History is unavailable.";
    case "store_too_new":
      return "This history was saved by a newer Kelvo version.";
  }
}

export function isHostKeyError(error: CommandError): boolean {
  return error.kind === "host_key_unknown";
}
