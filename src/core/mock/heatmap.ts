/**
 * Mock `query_heatmap`: the Timeline's "Last 30 days, by hour" sample
 * data. Weekdays are busy from
 * 09:00 to 19:00 with a bump at 14:00 and 15:00, evenings are lighter,
 * nights (02:00 to 06:59, and 00:00 and 01:00 on about half the nights) are
 * asleep and `null` (hatched), and one day has a long afternoon spike.
 *
 * Seeded per local date rather than once for the whole run, so a day keeps
 * its values whichever range asks for it. Hours that start after `nowMs`
 * and empty cells (a spring-forward hour) are `null`, as Rust returns them.
 */
import type {
  HeatmapDay,
  HeatmapMetric,
  HeatmapRequest,
} from "@core/generated/bindings";
import { fnv1a, rng } from "./rng";

/** FNV-1a of `s`, as a Lehmer seed (1 to 2^31 - 2). */
function hash(s: string): number {
  return (fnv1a(s) % 2147483646) + 1;
}

/** Average CPU % per local hour of `date` (`YYYY-MM-DD`), by the rules above. */
export function mockCpuHours(date: string): (number | null)[] {
  const [y, m, d] = date.split("-").map(Number);
  const weekday = new Date(y ?? 1970, (m ?? 1) - 1, d ?? 1).getDay();
  const weekend = weekday === 0 || weekday === 6;
  const r = rng(hash(date));
  // A busy day: one in about 30, picked by date.
  const spike = hash(`${date}#spike`) % 30 === 0;
  const hours: (number | null)[] = [];
  for (let h = 0; h < 24; h++) {
    const awake = h >= 7 || (h <= 1 && r() < 0.55);
    let v: number | null = null;
    if (awake) {
      if (!weekend && h >= 9 && h < 19)
        v = 20 + r() * 38 + (h === 14 || h === 15 ? 10 : 0);
      else if (h >= 19) v = 7 + r() * 16;
      else if (h < 2) v = 4 + r() * 10;
      else v = weekend ? 4 + r() * 20 : 8 + r() * 12;
      if (spike && h >= 10 && h < 17) v = 60 + r() * 25;
    }
    hours.push(v === null ? null : Math.round(v * 10) / 10);
  }
  return hours;
}

/** CPU load to a hottest-zone temperature, °C. */
function cpuToTemp(v: number): number {
  return Math.round((38 + v * 0.6) * 10) / 10;
}

export function mockHeatmap(req: HeatmapRequest, nowMs: number): HeatmapDay[] {
  const metric: HeatmapMetric = req.metric;
  return req.days.map((day) => {
    const cpu = mockCpuHours(day.date);
    const hours = cpu.map((v, h) => {
      const start = day.hour_starts[h];
      const end = day.hour_starts[h + 1];
      if (
        v === null ||
        start === undefined ||
        end === undefined ||
        end <= start ||
        start > nowMs
      )
        return null;
      return metric === "temp" ? cpuToTemp(v) : v;
    });
    return { date: day.date, hours };
  });
}
