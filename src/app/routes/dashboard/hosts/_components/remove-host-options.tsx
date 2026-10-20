import { useId } from "react";
import { Checkbox } from "~/components/ui/checkbox";

export interface RemoveChoices {
  stopSharing: boolean;
  uninstallAgent: boolean;
  purgeAgent: boolean;
  deleteMirror: boolean;
}

function Choice({
  checked,
  disabled,
  onChange,
  children,
}: {
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
  children: string;
}) {
  const id = useId();
  return (
    <label htmlFor={id} className="flex items-center gap-2 text-[13px]">
      <Checkbox
        id={id}
        checked={checked}
        onCheckedChange={(next) => onChange(next === true)}
        disabled={disabled}
      />
      {children}
    </label>
  );
}

/** What removing also does on the host: a Mac stops sharing; a Linux agent is uninstalled (D-105). */
export function RemoveHostOptions({
  linux,
  choices,
  setChoices,
  busy,
}: {
  linux: boolean;
  choices: RemoveChoices;
  setChoices: (choices: RemoveChoices) => void;
  busy: boolean;
}) {
  const set = (patch: Partial<RemoveChoices>) =>
    setChoices({ ...choices, ...patch });
  return (
    <div className="grid gap-3">
      {linux ? (
        <>
          <Choice
            checked={choices.uninstallAgent}
            disabled={busy}
            onChange={(uninstallAgent) =>
              set({
                uninstallAgent,
                purgeAgent: uninstallAgent && choices.purgeAgent,
              })
            }
          >
            Also uninstall the agent from that server
          </Choice>
          <Choice
            checked={choices.purgeAgent}
            disabled={busy || !choices.uninstallAgent}
            onChange={(purgeAgent) => set({ purgeAgent })}
          >
            Delete the agent’s history on that server
          </Choice>
        </>
      ) : (
        <Choice
          checked={choices.stopSharing}
          disabled={busy}
          onChange={(stopSharing) => set({ stopSharing })}
        >
          Also stop sharing on that Mac
        </Choice>
      )}
      <Choice
        checked={choices.deleteMirror}
        disabled={busy}
        onChange={(deleteMirror) => set({ deleteMirror })}
      >
        Delete mirrored history from this Mac
      </Choice>
    </div>
  );
}
