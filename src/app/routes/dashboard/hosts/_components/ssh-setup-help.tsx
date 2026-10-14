import { hostKeys } from "@core/query-keys";
import { sshTestCommand, sshVerificationCommand } from "@core/remote-errors";
import { unwrap } from "@core/transport";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Button } from "~/components/ui/button";
import { useTransport } from "~/lib/transport-context";

function Command({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const [error, setError] = useState(false);
  return (
    <div className="grid gap-1">
      <pre className="select-text whitespace-pre-wrap break-all rounded-control bg-muted p-2 font-sans text-[12px]">
        {text}
      </pre>
      <div className="flex items-center gap-2">
        <Button
          size="sm"
          variant="ghost"
          onClick={async () => {
            try {
              await navigator.clipboard.writeText(text);
              setCopied(true);
              setError(false);
            } catch {
              setError(true);
            }
          }}
        >
          {copied ? "Copy again" : "Copy"}
        </Button>
        <span role="status">
          {error
            ? "Could not copy. Select the text and copy it manually."
            : copied
              ? "Copied"
              : ""}
        </span>
      </div>
    </div>
  );
}

// biome-ignore lint/complexity/noExcessiveCognitiveComplexity: Controller and presentation split tracked in architecture refactor package H.
export function SshSetupHelp({
  lan,
  tailscale,
  busy,
  saved = false,
}: {
  lan: string | null;
  tailscale: string | null;
  busy: boolean;
  saved?: boolean;
}) {
  const transport = useTransport();
  const config = useQuery({
    queryKey: hostKeys.sshConfig(),
    queryFn: () => unwrap(transport.remoteSshConfigStatus()),
    retry: false,
    staleTime: 0,
  });
  const [expanded, setExpanded] = useState<boolean | null>(null);
  const open = expanded ?? config.data === "missing";
  const aliases = [
    ...new Set([lan, tailscale].filter((value): value is string => !!value)),
  ];
  return (
    <div className="grid gap-2 border-border-subtle border-y py-3 text-[12px]">
      <p className="text-muted-foreground" role="status">
        {config.isPending
          ? "Checking SSH configuration on this Mac…"
          : config.isError
            ? "Could not check ~/.ssh/config. You can still test your connection."
            : config.data === "missing"
              ? "No ~/.ssh/config found on this Mac. You can use user@address directly, or set up an alias below."
              : "Found ~/.ssh/config on this Mac. Finding this file does not confirm SSH access."}
      </p>
      <div className="flex gap-2">
        <Button
          size="sm"
          variant="ghost"
          aria-expanded={open}
          onClick={() => setExpanded(!open)}
        >
          {open ? "Hide SSH setup guide" : "Show SSH setup guide"}
        </Button>
        <Button
          size="sm"
          variant="ghost"
          disabled={busy || config.isFetching}
          onClick={() => config.refetch()}
        >
          Recheck config
        </Button>
      </div>
      {config.isError && (
        <details className="text-muted-foreground">
          <summary>Configuration check details</summary>
          <p className="select-text break-words">{String(config.error)}</p>
        </details>
      )}
      {open && (
        <ol className="ml-4 grid list-decimal gap-3 text-fg-subtle">
          <li>
            On the other Mac, open System Settings → General → Sharing → Remote
            Login. Turn it on and allow the account signed in to that Mac’s
            desktop. Note the username and address shown there.
          </li>
          <li>
            On this Mac, set up SSH key access for that account. If you already
            use an SSH key or password manager’s SSH agent, use that key.
            <details className="mt-1">
              <summary>Set up a new SSH key</summary>
              <p className="my-2">
                In Terminal on this Mac, run ssh-keygen -t ed25519 and follow
                the prompts. If it asks to overwrite an existing key, answer no
                and use that key or choose another filename.
              </p>
              <p>Copy the public key (use your key’s path if different):</p>
              <Command text="pbcopy < ~/.ssh/id_ed25519.pub" />
              <p>
                On the other Mac, sign in as the account you want to monitor. In
                Terminal, run mkdir -p ~/.ssh, then nano ~/.ssh/authorized_keys.
                Paste the public key on its own line, keeping existing keys.
                Save with Control-O, Enter, then exit with Control-X. Set the
                permissions:
              </p>
              <Command text="chmod 700 ~/.ssh && chmod 600 ~/.ssh/authorized_keys" />
              <p>
                Transfer only the .pub text; the private key stays on this Mac.
              </p>
            </details>
            <p className="mt-1">
              If your key has a passphrase, unlock it with{" "}
              <span className="select-text">ssh-add ~/.ssh/id_ed25519</span>{" "}
              (use your key’s path). Kelvo connects without password prompts.
            </p>
          </li>
          <li>
            Optional: create or edit ~/.ssh/config on this Mac. Create the
            ~/.ssh folder first if needed. Add this example, replacing the
            address, username, and key path with yours. Keep any existing
            entries. In Terminal, use mkdir -p ~/.ssh, then nano ~/.ssh/config.
            Save with Control-O, Enter, then exit with Control-X.
            <Command
              text={
                "Host studio\n    HostName studio.local\n    User your-username\n    IdentityFile ~/.ssh/id_ed25519"
              }
            />
            <p>
              Then enter studio in LAN SSH alias. You can also enter
              your-username@studio.local directly. Leave Tailscale blank unless
              you use it; its alias should reach the same Mac and account.
            </p>
          </li>
          <li>
            {saved
              ? "Save any address changes, then use Retry in Hosts. If SSH needs host verification, follow the Terminal steps below."
              : "Use Check connection below. For an unknown host, use Verify host and compare the fingerprint with the other Mac before trusting it."}{" "}
            {saved && (
              <div className="my-2 grid gap-2">
                <p>
                  On the other Mac, run this in Terminal to get its
                  fingerprints:
                </p>
                <Command
                  text={
                    'for key in /etc/ssh/ssh_host_*_key.pub; do ssh-keygen -lf "$key"; done'
                  }
                />
                <p>
                  On this Mac, run the verification command for the address you
                  changed. At the trust prompt, compare its SHA256 fingerprint
                  with the matching key type above. Answer yes only if they
                  match. If the key changed, stop and confirm why with the
                  owner; do not remove a known key just to connect.
                </p>
                {(aliases.length ? aliases : ["studio"]).map((alias) => {
                  const command = sshVerificationCommand(alias);
                  return command ? (
                    <Command key={alias} text={command} />
                  ) : null;
                })}
                <p>
                  This command can prompt for a password or key passphrase.
                  After verifying the host, run the separate test below to check
                  the access Kelvo needs without prompts.
                </p>
              </div>
            )}
            To test without password or trust prompts in Terminal on this Mac,
            run:
            {(aliases.length ? aliases : ["studio"]).map((alias) => {
              const command = sshTestCommand(alias);
              return command ? (
                <Command key={alias} text={command} />
              ) : (
                <p key={alias}>
                  Enter only an SSH alias or user@address, without “ssh” or
                  command options.
                </p>
              );
            })}
            <p>
              A successful test exits without output.{" "}
              {saved
                ? "Return to Hosts and retry the connection."
                : "Return here and check the connection again."}
            </p>
          </li>
        </ol>
      )}
    </div>
  );
}
