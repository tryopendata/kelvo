import { chartRange, relativeWindow, validChartRange } from "@core/chart-range";
import type { ChartWindow } from "@core/generated/bindings";
import { useCallback, useEffect, useRef, useState } from "react";
import { useLocation, useSearchParams } from "react-router";
import { useRangeNavigation } from "~/hooks/use-range-navigation";
import { useSettingsStore } from "~/stores/settings-store";
import {
  DOCKER_RANGES,
  type DockerRange,
  dockerRange,
  historyEnd,
} from "../_lib/history-params";

/** Incoming links apply once; subsequent investigations follow the shared saved range. */
export function useSharedContainerRange(nowMs: number, prefix = "") {
  const navigation = useRangeNavigation();
  const { apply, range } = navigation;
  const settings = useSettingsStore();
  const [params, setParams] = useSearchParams();
  const { key, state } = useLocation();
  const attempted = useRef("");
  // A retained-browser link is more specific than legacy main-chart parameters.
  const retainedLink = params.has("retainedRange") || params.has("retainedEnd");
  const linkPrefix = retainedLink ? "retained" : prefix;
  const rangeKey = linkPrefix ? `${linkPrefix}Range` : "range";
  const endKey = linkPrefix ? `${linkPrefix}End` : "end";
  const incoming =
    params.has(rangeKey) ||
    params.has(endKey) ||
    (!linkPrefix && params.has("at"));
  const rawRange = params.get(rangeKey);
  const rawEnd = params.get(endKey);
  const rawAt = linkPrefix ? null : params.get("at");
  const signature = `${key}:${rawRange}:${rawEnd}:${rawAt}`;
  const [applyingLink, setApplyingLink] = useState(incoming);
  useEffect(() => {
    if (!incoming) {
      setApplyingLink(false);
      return;
    }
    if (!range || navigation.pending) return;
    if (attempted.current === signature) return;
    attempted.current = signature;
    setApplyingLink(true);
    const next = containerLinkRange(rawRange, rawEnd, rawAt, linkPrefix);
    if (!next) {
      setApplyingLink(false);
      return;
    }
    void apply(next).then(() => {
      setApplyingLink(false);
      const saved = settings.getState().snapshot?.settings.general.chart_window;
      if (
        !saved ||
        JSON.stringify(chartRange(saved)) !== JSON.stringify(chartRange(next))
      )
        return;
      const consumed = retainedLink
        ? ["retainedRange", "retainedEnd", "range", "end"]
        : [rangeKey, endKey, ...(linkPrefix ? [] : ["at"])];
      setParams((current) => consumeRangeLink(current, params, consumed), {
        replace: true,
        state,
      });
    });
  }, [
    incoming,
    navigation.pending,
    signature,
    rawRange,
    rawEnd,
    rawAt,
    rangeKey,
    endKey,
    linkPrefix,
    retainedLink,
    range,
    apply,
    settings,
    setParams,
    params,
    state,
  ]);
  const windowMs = navigation.range?.windowMs ?? 900_000;
  const endMs = navigation.range?.endMs ?? null;
  const toMs = endMs ?? Math.floor(nowMs / 10_000) * 10_000;
  const preset =
    (Object.keys(DOCKER_RANGES) as DockerRange[]).find(
      (key) => DOCKER_RANGES[key] === windowMs
    ) ?? "15m";
  const setRange = useCallback(
    (duration: number) =>
      void apply(
        endMs === null
          ? relativeWindow(duration)
          : { absolute: { from_ms: endMs - duration, to_ms: endMs } }
      ),
    [apply, endMs]
  );
  const setEnd = useCallback(
    (end: number | null) =>
      void apply(
        end === null
          ? relativeWindow(windowMs)
          : { absolute: { from_ms: end - windowMs, to_ms: end } }
      ),
    [apply, windowMs]
  );
  return {
    ready:
      range !== null &&
      !applyingLink &&
      (!incoming || attempted.current === signature),
    windowMs,
    endMs,
    fromMs: toMs - windowMs,
    toMs,
    preset,
    setRange,
    setEnd,
  };
}

function containerLinkRange(
  rawRange: string | null,
  rawEnd: string | null,
  rawAt: string | null,
  prefix: string
): ChartWindow | null {
  const duration = DOCKER_RANGES[dockerRange(rawRange, prefix ? "1h" : "15m")];
  const end = historyEnd(rawEnd) ?? historyEnd(rawAt, 60_000);
  const next: ChartWindow =
    end === null
      ? relativeWindow(duration)
      : { absolute: { from_ms: end - duration, to_ms: end } };
  return validChartRange(next) ? next : null;
}

function consumeRangeLink(
  current: URLSearchParams,
  original: URLSearchParams,
  keys: readonly string[]
) {
  if (keys.some((key) => current.get(key) !== original.get(key)))
    return current;
  const result = new URLSearchParams(current);
  for (const key of keys) result.delete(key);
  return result;
}
