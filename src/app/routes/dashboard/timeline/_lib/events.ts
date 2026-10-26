import { eventId } from "@core/events";
import type { Event } from "@core/generated/bindings";
import { selectedRange } from "./selection";

export function eventPriority(event: Event): number {
  const detail = event.detail;
  if (detail.kind === "alert") return 3;
  if (detail.kind === "thermal_state") {
    const states = ["nominal", "fair", "serious", "critical"];
    return states.indexOf(detail.to) > states.indexOf(detail.from ?? "") &&
      ["serious", "critical"].includes(detail.to)
      ? 3
      : 0;
  }
  return detail.kind === "fans_ramped" ? 1 : 2;
}

export interface EventGroup {
  id: string;
  representative: Event;
  members: Event[];
  fraction: number;
}

/** Screen-space clusters preserve every identity; zooming naturally separates them. */
export function groupEvents(
  events: readonly Event[],
  fromMs: number,
  toMs: number,
  widthPx: number
): EventGroup[] {
  if (widthPx <= 0 || toMs <= fromMs) return [];
  const groups: EventGroup[] = [];
  const threshold = 40 / widthPx;
  for (const event of events
    .filter((event) => event.ts_ms >= fromMs && event.ts_ms < toMs)
    .sort((a, b) => a.ts_ms - b.ts_ms)) {
    const fraction = Math.min(
      1,
      Math.max(0, (event.ts_ms - fromMs) / (toMs - fromMs))
    );
    const group = groups.at(-1);
    if (group && fraction - group.fraction < threshold) {
      group.members.push(event);
      if (eventPriority(event) > eventPriority(group.representative))
        group.representative = event;
    } else
      groups.push({
        id: eventId(event),
        representative: event,
        members: [event],
        fraction,
      });
  }
  return groups;
}

export function eventRange(
  events: readonly Event[],
  bucketMs: number,
  nowMs: number,
  retainedFrom = 0
) {
  if (events.length === 0) return null;
  const start = Math.min(
    ...events.map((event) => Math.min(event.start_ms, event.ts_ms))
  );
  const detection = Math.max(...events.map((event) => event.ts_ms));
  const padding =
    detection === start ? 300_000 : Math.max(60_000, (detection - start) * 0.2);
  return selectedRange(
    start - padding,
    detection + padding,
    bucketMs,
    nowMs,
    retainedFrom
  );
}
