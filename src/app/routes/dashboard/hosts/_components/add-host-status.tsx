import type {
  CommandError,
  RemoteHostPreparation,
} from "@core/generated/bindings";
import { AddHostLinux } from "./add-host-linux";
import { RemoteSetupError } from "./remote-setup-error";
import type { SetupPhase } from "./use-add-host-connection";

function macStatus(prepared: RemoteHostPreparation): string {
  return (
    prepared.message ??
    (prepared.installed
      ? prepared.compatible
        ? "Kelvo is ready to connect."
        : "Kelvo needs an update. The installed app will be replaced after verification."
      : "Kelvo will be installed in your Applications folder on the other Mac.")
  );
}

function busyStatus(phase: SetupPhase, prepared: RemoteHostPreparation | null) {
  if (phase === "cancelling") return "Cancelling… waiting for setup cleanup.";
  if (phase === "verifying") return "Verifying SSH host…";
  if (phase === "checking") return "Checking SSH access and Kelvo…";
  if (phase === "resetting") return "Giving the agent a new identity…";
  if (prepared?.kind === "linux")
    return prepared.installed && prepared.compatible
      ? "Connecting to the agent…"
      : "Installing the agent and connecting…";
  return "Setting up Kelvo and connecting…";
}

export function AddHostStatus({
  endpointError,
  prepared,
  error,
  busy,
  phase,
}: {
  endpointError: string | null;
  prepared: RemoteHostPreparation | null;
  error: CommandError | string | null;
  busy: boolean;
  phase: SetupPhase;
}) {
  return (
    <>
      {" "}
      {endpointError && (
        <p role="alert" className="text-[12px] text-destructive">
          {endpointError}
        </p>
      )}
      {prepared &&
        !error &&
        (prepared.linux ? (
          <AddHostLinux prepared={prepared} linux={prepared.linux} />
        ) : (
          <div className="grid gap-1 text-[13px] text-fg-subtle">
            <p>{macStatus(prepared)}</p>
            <p>
              Kelvo on that Mac will start at login and share its metrics with
              this Mac.
            </p>
          </div>
        ))}
      {busy && (
        <p role="status" className="text-[13px] text-fg-subtle">
          {busyStatus(phase, prepared)}
        </p>
      )}
      {error && <RemoteSetupError error={error} />}
    </>
  );
}
