import type { CardDensity, PopoverSettings } from "@core/generated/bindings";
import { useCallback } from "react";
import { toast } from "sonner";
import { useSettings, useUpdateSettings } from "~/stores/settings-store";

export const POPOVER_CARDS = [
  "cpu",
  "memory",
  "gpu",
  "power",
  "network",
  "battery",
  "containers",
] as const;
export type DisplayCard = (typeof POPOVER_CARDS)[number];

/** Persist only the changed fields; another card or window keeps its preferences. */
export function usePopoverDensity() {
  const saved = useSettings((s) => s.popover);
  const update = useUpdateSettings();
  const write = useCallback(
    (patch: Partial<PopoverSettings>) => {
      const save = async () => {
        const result = await update({ popover: patch }).catch(
          (error: unknown) => {
            console.error("[popover] options save failed", error);
            return null;
          }
        );
        if (result?.status === "ok") toast.dismiss("popover-density");
        else
          toast.error("Popover options could not be saved.", {
            id: "popover-density",
            action: { label: "Retry", onClick: () => void save() },
          });
      };
      void save();
    },
    [update]
  );
  return {
    compact: (card: DisplayCard) => saved?.[card] === "compact",
    toggle: (card: DisplayCard) =>
      write({ [card]: saved?.[card] === "compact" ? "expanded" : "compact" }),
    setAll: (density: CardDensity) =>
      write(Object.fromEntries(POPOVER_CARDS.map((card) => [card, density]))),
  };
}
