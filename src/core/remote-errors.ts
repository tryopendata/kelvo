import type { CommandError } from "@core/generated/bindings";
import { CommandFailure } from "./transport/query";

export function remoteErrorText(error: CommandError): string {
  if ("message" in error) return error.message;
  switch (error.kind) {
    case "unknown_host":
      return "This host is no longer in your host list.";
    case "remote_host":
      return "This host does not share this data.";
    case "history_unavailable":
      return "History is unavailable.";
    case "store_too_new":
      return "This history was saved by a newer Kelvo version.";
  }
}

/**
 * Why a remote host did not answer an on-demand query (D-117), such as a
 * space report or use by app: `what` names the data ("space reports").
 * `null` for any other failure, which the caller words itself.
 */
export function remoteQueryText(error: unknown, what: string): string | null {
  if (!(error instanceof CommandFailure)) return null;
  switch (error.error.kind) {
    case "remote_incompatible":
      return `This host's Kelvo is too old to share ${what}. Update Kelvo on it, then try again.`;
    case "remote_unreachable":
      return `This host isn't connected, so its ${what} can't be read now.`;
    case "remote_busy":
      return `This host is busy or slow to answer, so its ${what} can't be read now. Try again in a moment.`;
    case "remote_unsupported":
      return `This host doesn't offer ${what}.`;
    default:
      return null;
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
          "Set up SSH key access for the account. On a Mac, use the account signed in there and allow it in Remote Login. Kelvo cannot ask for a password or key passphrase; unlock your key in your SSH agent, then check the connection again.",
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
        title: "SSH needs to verify this host.",
        recovery:
          "Compare its fingerprint with the host before trusting it. If SSH says the key changed, confirm why with the owner and resolve it in Terminal; Kelvo will not replace a known key.",
      };
    case "remote_incompatible":
      return {
        title: "This host cannot run this Kelvo build.",
        recovery:
          "A Mac needs Apple Silicon, macOS 26 or newer and a compatible Kelvo version. A Linux server needs a 64-bit ARM or x86 processor and systemd. See the details for what needs to change.",
      };
    case "remote_identity":
      return {
        title: "A different host answered at this address.",
        recovery:
          "Check the SSH alias, address, and account before trying again. If the host was reinstalled or replaced on purpose, add or re-pair it as a new host.",
      };
    case "remote_low_disk":
      return {
        title: "The server is low on disk space.",
        recovery:
          "The agent needs room for its history plus the reserve Kelvo keeps free. Free some space on the server, then check the connection again.",
      };
    case "remote_agent_conflict":
      return {
        title: "This server’s Kelvo agent needs attention first.",
        recovery: error.message,
      };
    case "remote_agent_not_running":
      return {
        title: "The Kelvo agent is not running on the server.",
        recovery:
          "Check it on the server with systemctl status kelvo-agent (add --user for a user service), then retry. The service restarts on its own after most failures.",
      };
    case "remote_agent_update_failed":
      return error.restored
        ? {
            title:
              "The agent update failed. The previous version was restored.",
            recovery:
              "Its history and identity are unchanged and it is running again. See the details, then retry the update.",
          }
        : {
            title:
              "The agent update failed, and the previous version could not be restored.",
            recovery:
              "The agent may not be running. See the details, then retry the update or add the host again to reinstall it.",
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
