import type {
  CommandError,
  RemoteHostPreparation,
} from "@core/generated/bindings";
import { RemoteSetupError } from "./remote-setup-error";
import type { SetupPhase } from "./use-add-mac-connection";
export function AddMacStatus({
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
      {prepared && !error && (
        <p className="text-[13px] text-fg-subtle">
          {prepared.message ??
            (prepared.installed
              ? prepared.compatible
                ? "Kelvo is ready to connect."
                : "Kelvo needs an update. The installed app will be replaced after verification."
              : "Kelvo will be installed in your Applications folder on the other Mac.")}
        </p>
      )}
      {busy && (
        <p role="status" className="text-[13px] text-fg-subtle">
          {phase === "cancelling"
            ? "Cancelling… waiting for setup cleanup."
            : phase === "verifying"
              ? "Verifying SSH host…"
              : phase === "checking"
                ? "Checking SSH access and Kelvo…"
                : "Setting up Kelvo and connecting…"}
        </p>
      )}
      {error && <RemoteSetupError error={error} />}
    </>
  );
}
