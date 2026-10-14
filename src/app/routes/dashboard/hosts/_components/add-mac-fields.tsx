import type {
  CommandError,
  RemoteHostPreparation,
  RemoteSshHostTrust,
} from "@core/generated/bindings";
import { Switch } from "~/components/ui/switch";
import { HOST_INPUT } from "./host-input";
export function AddMacFields({
  busy,
  name,
  setName,
  lan,
  setLan,
  tailscale,
  setTailscale,
  showMenuBar,
  setShowMenuBar,
  setPrepared,
  setTrust,
  setHostKeyError,
  setError,
}: {
  busy: boolean;
  name: string;
  setName: (value: string) => void;
  lan: string;
  setLan: (value: string) => void;
  tailscale: string;
  setTailscale: (value: string) => void;
  showMenuBar: boolean;
  setShowMenuBar: (value: boolean) => void;
  setPrepared: (value: RemoteHostPreparation | null) => void;
  setTrust: (value: RemoteSshHostTrust | null) => void;
  setHostKeyError: (value: boolean) => void;
  setError: (value: CommandError | string | null) => void;
}) {
  return (
    <fieldset disabled={busy} className="grid gap-3">
      <label className="grid gap-1 text-[12px]">
        Name
        <input
          className={HOST_INPUT}
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Studio Mac"
        />
      </label>
      <label className="grid gap-1 text-[12px]">
        LAN SSH alias
        <input
          className={HOST_INPUT}
          value={lan}
          onChange={(e) => {
            setLan(e.target.value);
            setPrepared(null);
            setTrust(null);
            setHostKeyError(false);
            setError(null);
          }}
          placeholder="studio or username@studio.local"
          autoCapitalize="none"
          spellCheck={false}
        />
      </label>
      <label className="grid gap-1 text-[12px]">
        Tailscale SSH alias
        <input
          className={HOST_INPUT}
          value={tailscale}
          onChange={(e) => {
            setTailscale(e.target.value);
            setPrepared(null);
            setTrust(null);
            setHostKeyError(false);
            setError(null);
          }}
          placeholder="studio-tailnet"
          autoCapitalize="none"
          spellCheck={false}
        />
      </label>
      <p className="text-[12px] text-muted-foreground">
        Enter an alias from ~/.ssh/config or user@address, without “ssh”. Use
        the account signed in on the other Mac. Only one address is needed.
      </p>
      <div className="flex items-center justify-between gap-3 text-[12px]">
        <span>Show menu-bar icon on the other Mac</span>
        <Switch
          aria-label="Show menu-bar icon on remote Mac"
          checked={showMenuBar}
          onCheckedChange={setShowMenuBar}
        />
      </div>
    </fieldset>
  );
}
