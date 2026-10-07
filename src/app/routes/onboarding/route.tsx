import type { SettingsPatch } from "@core/generated/bindings";
import {
  type OnboardingChoices,
  onboardingChoicesPatch,
  onboardingDonePatch,
  onboardingSkipPatch,
  settingsErrorText,
} from "@core/settings-patch";
import { useState } from "react";
import { toast } from "sonner";
import { useHostRecord } from "~/hooks/use-host-record";
import { useTransport } from "~/lib/transport-context";
import { useSettings, useUpdateSettings } from "~/stores/settings-store";
import { PrivacyStep } from "./_components/privacy-step";
import { SetupStep } from "./_components/setup-step";

/**
 * Onboarding window (plan 4.16). Step 1 picks modules, the menu
 * bar style and launch at login; Continue saves them and shows step 2,
 * "Updates and privacy". Skip keeps the defaults; Skip and Done mark
 * onboarding completed and close the window.
 */
export default function OnboardingRoute() {
  const transport = useTransport();
  const update = useUpdateSettings();
  const host = useHostRecord();
  const general = useSettings((s) => s.general);
  const history = useSettings((s) => s.history);
  const [step, setStep] = useState<1 | 2>(1);
  const [busy, setBusy] = useState(false);

  /** Send a patch; true when it was saved. */
  const save = async (patch: SettingsPatch): Promise<boolean> => {
    setBusy(true);
    const result = await update(patch);
    setBusy(false);
    if (result.status === "error") {
      console.error("[onboarding] update_settings failed", {
        patch,
        error: result.error,
      });
      toast.error(settingsErrorText(result.error));
      return false;
    }
    return true;
  };

  const finish = async (patch: SettingsPatch) => {
    if (await save(patch)) await transport.closeWindow();
  };

  if (step === 2) {
    return (
      <PrivacyStep
        initialCheckUpdates={general?.check_updates ?? true}
        retentionDays={history?.retention_days ?? 30}
        sizeLimitMb={history?.size_limit_mb ?? 150}
        busy={busy}
        onDone={(checkUpdates) =>
          void finish(onboardingDonePatch(checkUpdates))
        }
      />
    );
  }
  return (
    <SetupStep
      hostInfo={host?.info}
      busy={busy}
      onSkip={() => void finish(onboardingSkipPatch())}
      onContinue={(choices: OnboardingChoices) =>
        void save(onboardingChoicesPatch(choices)).then((ok) => {
          if (ok) setStep(2);
        })
      }
    />
  );
}
