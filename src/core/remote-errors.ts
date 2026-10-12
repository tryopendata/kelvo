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

/** Recovery copy follows typed command errors; SSH's diagnostic is kept separately. */
export function remoteErrorAdvice(
  error: CommandError
): { title: string; recovery: string } | null {
  switch (error.kind) {
    case "remote_auth":
      return {
        title: "SSH could not sign in.",
        recovery:
          "Use the account currently signed in on the other Mac, allow it in Remote Login, and set up SSH key access. Kelvo cannot ask for a password or key passphrase; unlock your key in your SSH agent, then check the connection again.",
      };
    case "remote_unreachable":
      return {
        title: "Kelvo could not complete the connection setup.",
        // This variant also carries installation, protocol and local I/O failures.
        // Keep the actual cause visible instead of assuming a network failure.
        recovery: error.message,
      };
    case "host_key_unknown":
      return {
        title: "SSH needs to verify this Mac.",
        recovery:
          "Compare its fingerprint with the other Mac before trusting it. If SSH says the key changed, confirm why with the owner and resolve it in Terminal; Kelvo will not replace a known key.",
      };
    case "remote_incompatible":
      return {
        title: "This Mac cannot run this Kelvo build.",
        recovery:
          "Remote monitoring needs an Apple Silicon Mac running macOS 26 or newer and a compatible Kelvo version. See the details for what needs to change.",
      };
    case "remote_identity":
      return {
        title: "A different Mac answered at this address.",
        recovery:
          "Check the SSH alias, address, and account before trying again. Add it as a new Mac if the replacement was intentional.",
      };
    default:
      return null;
  }
}

/** Shell-quote display commands too: aliases can contain shell metacharacters. */
export function sshTestCommand(alias: string): string | null {
  return sshCommand(alias, "test");
}

export function sshVerificationCommand(alias: string): string | null {
  return sshCommand(alias, "verify");
}

function sshCommand(alias: string, mode: "test" | "verify"): string | null {
  const value = alias.trim();
  if (!value || sshEndpointError(value)) return null;
  const options =
    mode === "verify"
      ? "-o BatchMode=no -o StrictHostKeyChecking=ask"
      : "-o BatchMode=yes -o StrictHostKeyChecking=yes";
  return `/usr/bin/ssh ${options} -o ConnectTimeout=5 -o ControlPath=none -- '${value.replaceAll("'", "'\\''")}' true`;
}

export function sshEndpointError(value: string): string | null {
  if (!value.trim()) return null;
  if (
    value.trim().startsWith("-") ||
    /[\s\p{Cc}]/u.test(value.trim()) ||
    value.trim().length > 255
  ) {
    return "Enter an SSH alias or user@address, without “ssh”, spaces, or command options (up to 255 characters).";
  }
  return null;
}
