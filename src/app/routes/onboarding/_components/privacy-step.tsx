import {
  approxSize,
  DEFAULT_SERIES,
  retentionProjection,
  sizeLimitLabel,
} from "@core/history-projection";
import { useState } from "react";
import { Button } from "~/components/ui/button";
import { Switch } from "~/components/ui/switch";
import { useSeriesCount } from "~/stores/live-selectors";
import { FIELD_LABEL } from "~/widgets/lib/classes";
import { OnboardingFrame } from "./onboarding-frame";

const HISTORY_DIR = "~/Library/Application Support/com.tryopendata.kelvo/";

/**
 * Step 2 of 2, "Updates and privacy" (plan 4.16, D-028; it reuses step
 * 1's frame, list well and controls). One switch, then a
 * plain statement about telemetry and where history lives.
 */
export function PrivacyStep({
  initialCheckUpdates,
  retentionDays,
  sizeLimitMb,
  busy,
  onDone,
}: {
  initialCheckUpdates: boolean;
  retentionDays: number;
  sizeLimitMb: number;
  busy: boolean;
  onDone: (checkUpdates: boolean) => void;
}) {
  const [checkUpdates, setCheckUpdates] = useState(initialCheckUpdates);
  const series = useSeriesCount() ?? DEFAULT_SERIES;
  const projection = retentionProjection(
    retentionDays,
    sizeLimitMb * 1e6,
    series
  );
  const sizeText =
    projection.limitedDays === null
      ? `takes about ${approxSize(projection.bytes)} for ${retentionDays} days`
      : `stays under ${sizeLimitLabel(sizeLimitMb)}, about ${projection.limitedDays} days`;

  return (
    <OnboardingFrame
      step={2}
      title="Updates and privacy"
      sub="Kelvo works without a network connection. You decide whether it checks for updates."
      footer={
        <>
          <span className="flex-1" />
          <Button disabled={busy} onClick={() => onDone(checkUpdates)}>
            Done
          </Button>
        </>
      }
    >
      <div className="flex max-w-[560px] flex-col gap-6">
        <div className="flex flex-col gap-2">
          <span className={FIELD_LABEL}>Updates</span>
          <div className="flex items-center gap-2.5 rounded-tile border border-border bg-well px-3 py-2.5">
            <div className="flex min-w-0 flex-1 flex-col gap-0.5">
              <label htmlFor="onboarding-updates" className="text-[13px]">
                Check for updates automatically
              </label>
              <span className="font-normal text-[11px] text-muted-foreground">
                This is the only network request Kelvo makes. You can turn it
                off any time in Settings.
              </span>
            </div>
            <Switch
              id="onboarding-updates"
              checked={checkUpdates}
              onCheckedChange={setCheckUpdates}
            />
          </div>
        </div>
        <div className="flex flex-col gap-2">
          <span className={FIELD_LABEL}>Privacy</span>
          <p className="font-normal text-[13px] text-fg-subtle leading-relaxed">
            Kelvo has no telemetry. All history stays on this Mac, in{" "}
            <span className="data-mono whitespace-nowrap text-[12px] text-foreground">
              {HISTORY_DIR}
            </span>
            , and {sizeText}.
          </p>
        </div>
      </div>
    </OnboardingFrame>
  );
}
