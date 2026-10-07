/**
 * Live state for one host and the reducer that applies `LiveMsg`s to it.
 * Plain TypeScript; the zustand store in src/app/stores wraps it.
 *
 * Current numbers come from the frame's `held` array (D-047, D-049): the
 * latest value of each series while it is still current, `null` once stale.
 * Chart lines come from raw rows (`values`, backfill rows), kept for one
 * hour twice: as rows (`rows`, for readers that walk whole rows) and as one
 * typed column per series (`columns`, for charts and window statistics, so a
 * tick costs one write per series and a window read is a scan of one
 * Float32Array, not a key lookup per row). Both are appended in place (a 1 Hz
 * tick must not copy an hour of history), so readers subscribe to
 * `rowsVersion` and read them.
 *
 * History arrives in two phases (D-066): `backfill` (the last two minutes)
 * and frames are appended; `backfill_earlier` chunks, newest first, are
 * prepended in front of the oldest row. A frame or backfill on another
 * `timeline` than the one held means the host's wall clock was stepped
 * (D-064): the rows at or after its first row are dropped and it goes on
 * from there; older rows stay. Both bump `rowsEpoch`, so caches keyed on row
 * times (closed chart buckets) recompute.
 */
import type {
  Capabilities,
  HostId,
  LiveMsg,
  LiveProcess,
  LiveStatus,
  MetricKind,
  SeriesKey,
} from "@core/generated/bindings";
import { RING_MAX_ROWS } from "@core/generated/bindings";
import { seriesKeyString } from "@core/series-key";

export interface LayoutInfo {
  no: number;
  series: SeriesKey[];
  /** Display-form keys in value order. */
  keys: string[];
  index: Map<string, number>;
  /** Display-form keys per metric id, in value order. */
  byMetric: Map<string, string[]>;
  /** Catalog kind per display key (D-090); absent is a gauge. */
  kinds: Map<string, MetricKind>;
}

/**
 * How long each series' sample stays current, by display key, as the host
 * published it for a run of rows (D-090). Shared by every row it covers.
 */
export type Holds = ReadonlyMap<string, number>;

const NO_HOLDS: Holds = new Map();

function holdsFor(layout: LayoutInfo, holdsMs: readonly number[]): Holds {
  return new Map(layout.keys.map((k, i) => [k, holdsMs[i] ?? 0]));
}

export interface LiveRow {
  tsMs: number;
  layoutNo: number;
  values: readonly (number | null)[];
}

/** The engine ring's rows (`RING_MAX_ROWS`: an hour at 0.5 s), oldest first. */
export class RowRing {
  private rows: LiveRow[] = [];
  constructor(readonly capacity: number = RING_MAX_ROWS) {}

  push(row: LiveRow): void {
    const last = this.rows[this.rows.length - 1];
    // A resumed channel only backfills what it missed (D-049); drop anything
    // that is not newer than what we hold.
    if (last && row.tsMs <= last.tsMs) return;
    this.rows.push(row);
    if (this.rows.length > this.capacity * 1.25) {
      this.rows = this.rows.slice(-this.capacity);
    }
  }

  /**
   * Put `older` (oldest first) in front of the held rows. Rows not older
   * than the oldest held row are dropped, and so are the oldest of them when
   * the ring would overflow: the newest history wins.
   */
  prepend(older: readonly LiveRow[]): void {
    const first = this.rows[0];
    let end = older.length;
    if (first) {
      while (end > 0 && (older[end - 1] as LiveRow).tsMs >= first.tsMs) end--;
    }
    const room = Math.max(0, this.capacity - this.rows.length);
    const start = Math.max(0, end - room);
    if (start >= end) return;
    this.rows = older.slice(start, end).concat(this.rows);
  }

  get length(): number {
    return this.rows.length;
  }

  /** Drop the rows at or after `tsMs`; returns how many went. */
  truncateFrom(tsMs: number): number {
    let lo = 0;
    let hi = this.rows.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if ((this.rows[mid] as LiveRow).tsMs >= tsMs) hi = mid;
      else lo = mid + 1;
    }
    const removed = this.rows.length - lo;
    this.rows.length = lo;
    return removed;
  }

  last(): LiveRow | undefined {
    return this.rows[this.rows.length - 1];
  }

  /** Rows with `tsMs > fromMs`, oldest first. */
  since(fromMs: number): LiveRow[] {
    let lo = 0;
    let hi = this.rows.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if ((this.rows[mid] as LiveRow).tsMs > fromMs) hi = mid;
      else lo = mid + 1;
    }
    return this.rows.slice(lo);
  }
}

/**
 * One hour of rows as a circular buffer of timestamps plus one Float32Array
 * per series key, `NaN` where the row did not measure the series (missing
 * from the row's layout, or `null`). Columns are created the first time a
 * key appears and back-filled with `NaN`. Appending a row writes every
 * column once: O(series), independent of the window any chart reads.
 *
 * Each row also keeps the holds the host published for it (D-090), so a
 * window can tell a sample taken less often than rows arrive from a gap.
 */
export class SeriesColumns {
  private readonly ts: Float64Array;
  private readonly holds: Holds[];
  private readonly cols = new Map<string, Float32Array>();
  /** Slot the next row is written to. */
  private head = 0;
  private size = 0;

  constructor(readonly capacity: number = RING_MAX_ROWS) {
    this.ts = new Float64Array(capacity);
    this.holds = new Array<Holds>(capacity).fill(NO_HOLDS);
  }

  get length(): number {
    return this.size;
  }

  /** Timestamp of the newest row, null when empty. */
  lastTsMs(): number | null {
    return this.size === 0 ? null : this.tsAt(this.size - 1);
  }

  /** Timestamp of the oldest row, null when empty. */
  firstTsMs(): number | null {
    return this.size === 0 ? null : this.tsAt(0);
  }

  /**
   * Append a row: `values[i]` is the value of `keys[i]`, and `holds` how
   * long each one stays current. A row not newer than the newest one is
   * dropped, like `RowRing.push`.
   */
  push(
    tsMs: number,
    keys: readonly string[],
    values: readonly (number | null)[],
    holds: Holds = NO_HOLDS
  ): void {
    const last = this.lastTsMs();
    if (last !== null && tsMs <= last) return;
    const slot = this.head;
    this.write(slot, tsMs, keys, values, holds);
    this.head = (slot + 1) % this.capacity;
    if (this.size < this.capacity) this.size++;
  }

  /**
   * Put a row in front of the oldest one (prepend a chunk newest row
   * first). Dropped when it is not older than the oldest row or the ring is
   * full, since the newest history wins. Returns whether it was written.
   */
  prepend(
    tsMs: number,
    keys: readonly string[],
    values: readonly (number | null)[],
    holds: Holds = NO_HOLDS
  ): boolean {
    if (this.size >= this.capacity) return false;
    const first = this.firstTsMs();
    if (first !== null && tsMs >= first) return false;
    const slot =
      (this.head - this.size - 1 + 2 * this.capacity) % this.capacity;
    this.write(slot, tsMs, keys, values, holds);
    this.size++;
    return true;
  }

  /** Drop the rows at or after `tsMs` (newest first); returns how many went. */
  truncateFrom(tsMs: number): number {
    let keep = this.size;
    while (keep > 0 && this.tsAt(keep - 1) >= tsMs) keep--;
    const removed = this.size - keep;
    this.head = (this.head - removed + this.capacity) % this.capacity;
    this.size = keep;
    return removed;
  }

  /** Series a row does not carry are a gap (`NaN`) at its slot. */
  private write(
    slot: number,
    tsMs: number,
    keys: readonly string[],
    values: readonly (number | null)[],
    holds: Holds
  ): void {
    this.ts[slot] = tsMs;
    this.holds[slot] = holds;
    for (const col of this.cols.values()) col[slot] = Number.NaN;
    for (let i = 0; i < keys.length; i++) {
      const key = keys[i] as string;
      let col = this.cols.get(key);
      if (!col) {
        col = new Float32Array(this.capacity).fill(Number.NaN);
        this.cols.set(key, col);
      }
      const v = values[i];
      col[slot] = v == null ? Number.NaN : v;
    }
  }

  /** Storage slot of logical row `i` (0 is the oldest held). */
  slot(i: number): number {
    return (this.head - this.size + i + this.capacity) % this.capacity;
  }

  /** Timestamp of logical row `i`. */
  tsAt(i: number): number {
    return this.ts[this.slot(i)] as number;
  }

  /** How long `key`'s sample in logical row `i` stays current; 0 if unknown. */
  holdAt(i: number, key: string): number {
    return (this.holds[this.slot(i)] as Holds).get(key) ?? 0;
  }

  /** The column for `key` (index it with `slot(i)`), undefined if never seen. */
  column(key: string): Float32Array | undefined {
    return this.cols.get(key);
  }

  /** Logical index of the first row with `tsMs > fromMs` (`length` if none). */
  firstAfter(fromMs: number): number {
    let lo = 0;
    let hi = this.size;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (this.tsAt(mid) > fromMs) hi = mid;
      else lo = mid + 1;
    }
    return lo;
  }
}

export type Connection = "connecting" | "live" | "reconnecting";

export interface HostLive {
  hostId: HostId;
  connection: Connection;
  capabilities: Capabilities | null;
  status: LiveStatus | null;
  layouts: Record<number, LayoutInfo>;
  layoutNo: number | null;
  /** The holds of the frames that follow (the last `holds` message). */
  frameHolds: Holds;
  /**
   * The host's clock timeline the held rows are on; a row on another one
   * replaces those at or after its time (D-064). Null before any row.
   */
  timeline: number | null;
  /** Latest value per series key (display form); absent or null is a gap. */
  held: Record<string, number | null>;
  /** Timestamp of the newest row or frame. */
  lastTsMs: number | null;
  /** Bumped whenever rows are added or the ring restarts. Charts subscribe to it. */
  rowsVersion: number;
  /**
   * Bumped when rows land anywhere but the end (earlier chunks) or the ring
   * restarts after a clock step. Caches of closed buckets key on it.
   */
  rowsEpoch: number;
  rows: RowRing;
  /** The same rows as one typed column per series key. */
  columns: SeriesColumns;
  processes: { tsMs: number; rows: LiveProcess[] } | null;
  /** No frame for three frame periods while not paused or display-idle (plan 4.17). */
  stale: boolean;
  /**
   * Time of the first frame since the sampling setup last changed (interval,
   * frame period, Performance mode, pause, display idle), so a measurement
   * of what the current setup costs skips rows sampled under the old one.
   * Null until the first frame after a change. A backfill (the window was
   * hidden, or the channel reconnected) and a clock step restart it too: the
   * rows it fills in were taken with this window not taking frames.
   */
  statusSinceMs: number | null;
}

export function initialHostLive(hostId: HostId): HostLive {
  return {
    hostId,
    connection: "connecting",
    capabilities: null,
    status: null,
    layouts: {},
    layoutNo: null,
    frameHolds: NO_HOLDS,
    timeline: null,
    held: {},
    lastTsMs: null,
    rowsVersion: 0,
    rowsEpoch: 0,
    rows: new RowRing(),
    columns: new SeriesColumns(),
    processes: null,
    stale: false,
    statusSinceMs: null,
  };
}

/** Whether `next` samples or renders differently from `prev`. */
function samplingChanged(prev: LiveStatus | null, next: LiveStatus): boolean {
  return (
    prev === null ||
    prev.interval_ms !== next.interval_ms ||
    prev.frame_period_ms !== next.frame_period_ms ||
    prev.performance !== next.performance ||
    prev.paused !== next.paused ||
    prev.display_idle !== next.display_idle
  );
}

export function layoutInfo(
  no: number,
  series: SeriesKey[],
  kinds: readonly MetricKind[] = []
): LayoutInfo {
  const keys = series.map(seriesKeyString);
  const index = new Map(keys.map((k, i) => [k, i]));
  const byMetric = new Map<string, string[]>();
  series.forEach((s, i) => {
    const list = byMetric.get(s.metric) ?? [];
    list.push(keys[i] as string);
    byMetric.set(s.metric, list);
  });
  const kindOf = new Map(
    kinds.map((k, i): [string, MetricKind] => [keys[i] as string, k])
  );
  return { no, series, keys, index, byMetric, kinds: kindOf };
}

function heldFrom(
  layout: LayoutInfo,
  values: readonly (number | null)[]
): Record<string, number | null> {
  const held: Record<string, number | null> = {};
  layout.keys.forEach((k, i) => {
    held[k] = values[i] ?? null;
  });
  return held;
}

/**
 * Apply one message. Returns a new state object (and a new `held` on frames)
 * so selectors see changes; the row ring is appended in place.
 */
export function reduceLive(state: HostLive, msg: LiveMsg): HostLive {
  switch (msg.kind) {
    case "caps":
      return { ...state, capabilities: msg.capabilities };

    case "status": {
      const { kind: _kind, ...status } = msg;
      return {
        ...state,
        status,
        // Paused or display-idle is not stale: nothing is expected.
        stale: status.paused || status.display_idle ? false : state.stale,
        statusSinceMs: samplingChanged(state.status, status)
          ? null
          : state.statusSinceMs,
      };
    }

    case "layout":
      return {
        ...state,
        layouts: {
          ...state.layouts,
          [msg.layout_no]: layoutInfo(msg.layout_no, msg.series, msg.kinds),
        },
        layoutNo: msg.layout_no,
      };

    case "backfill": {
      const layout = state.layouts[msg.layout_no];
      if (!layout) return state;
      state = onTimeline(state, msg.timeline, msg.start_ms);
      const holds = holdsFor(layout, msg.holds_ms);
      msg.rows.forEach((values, i) => {
        const tsMs = msg.start_ms + i * msg.interval_ms;
        state.rows.push({ tsMs, layoutNo: msg.layout_no, values });
        state.columns.push(tsMs, layout.keys, values, holds);
      });
      const last = state.rows.last();
      // Readouts before the first frame: the backfill's last row, for series
      // not already held. A frame replaces it within one tick.
      const lastRow = msg.rows[msg.rows.length - 1];
      const held = lastRow
        ? { ...heldFrom(layout, lastRow), ...nonNull(state.held) }
        : state.held;
      return {
        ...state,
        connection: "live",
        held,
        lastTsMs: last?.tsMs ?? state.lastTsMs,
        rowsVersion: state.rowsVersion + 1,
        statusSinceMs: null,
      };
    }

    case "backfill_earlier":
      return prependEarlier(state, msg);

    case "holds": {
      const layout = state.layouts[msg.layout_no];
      if (!layout) return state;
      return { ...state, frameHolds: holdsFor(layout, msg.holds_ms) };
    }

    case "frame": {
      const layout = state.layouts[msg.layout_no];
      if (!layout) return state;
      state = onTimeline(state, msg.timeline, msg.ts_ms);
      state.rows.push({
        tsMs: msg.ts_ms,
        layoutNo: msg.layout_no,
        values: msg.values,
      });
      state.columns.push(msg.ts_ms, layout.keys, msg.values, state.frameHolds);
      return {
        ...state,
        connection: "live",
        layoutNo: msg.layout_no,
        held: heldFrom(layout, msg.held),
        // A dropped duplicate leaves the newest row where it was.
        lastTsMs: state.columns.lastTsMs() ?? msg.ts_ms,
        rowsVersion: state.rowsVersion + 1,
        stale: false,
        statusSinceMs: state.statusSinceMs ?? msg.ts_ms,
      };
    }

    case "processes":
      return { ...state, processes: { tsMs: msg.ts_ms, rows: msg.rows } };

    default:
      return state;
  }
}

/**
 * Rows from `fromMs` on are on `timeline`. When that is not the timeline
 * held, the host's clock was stepped (D-064): the held rows at or after
 * `fromMs` belong to the old one and go. A row on the held timeline that is
 * not newer is a duplicate, and the caller drops it.
 */
function onTimeline(
  state: HostLive,
  timeline: number,
  fromMs: number
): HostLive {
  if (state.timeline === timeline) return state;
  if (state.timeline === null) return { ...state, timeline };
  state.rows.truncateFrom(fromMs);
  const removed = state.columns.truncateFrom(fromMs);
  if (removed === 0) return { ...state, timeline, statusSinceMs: null };
  return {
    ...state,
    timeline,
    statusSinceMs: null,
    lastTsMs: state.columns.lastTsMs(),
    rowsVersion: state.rowsVersion + 1,
    rowsEpoch: state.rowsEpoch + 1,
  };
}

/** Drop every row. Layouts, readouts and the subscription stay. */
export function clearRows<S extends HostLive>(state: S): S {
  return {
    ...state,
    rows: new RowRing(state.rows.capacity),
    columns: new SeriesColumns(state.columns.capacity),
    lastTsMs: null,
    rowsVersion: state.rowsVersion + 1,
    rowsEpoch: state.rowsEpoch + 1,
  };
}

/**
 * Older history (`backfill_earlier`, D-066), older than everything the
 * channel sent before it. Rows go in front of the oldest held row; rows that
 * overlap what is held, or no longer fit, are dropped.
 */
function prependEarlier(
  state: HostLive,
  msg: Extract<LiveMsg, { kind: "backfill_earlier" }>
): HostLive {
  const layout = state.layouts[msg.layout_no];
  if (!layout || msg.rows.length === 0) return state;
  const older: LiveRow[] = msg.rows.map((values, i) => ({
    tsMs: msg.start_ms + i * msg.interval_ms,
    layoutNo: msg.layout_no,
    values,
  }));
  const before = state.columns.length;
  const first = state.columns.firstTsMs();
  const holds = holdsFor(layout, msg.holds_ms);
  for (let i = older.length - 1; i >= 0; i--) {
    const row = older[i] as LiveRow;
    if (first !== null && row.tsMs >= first) continue;
    if (!state.columns.prepend(row.tsMs, layout.keys, row.values, holds)) {
      break;
    }
  }
  state.rows.prepend(older);
  if (state.columns.length === before) return state;
  return {
    ...state,
    lastTsMs: state.lastTsMs ?? state.columns.lastTsMs(),
    rowsVersion: state.rowsVersion + 1,
    rowsEpoch: state.rowsEpoch + 1,
  };
}

function nonNull(held: Record<string, number | null>) {
  const out: Record<string, number> = {};
  for (const [k, v] of Object.entries(held)) if (v !== null) out[k] = v;
  return out;
}

/**
 * How far apart the rows a window receives are: the interval, or the frame
 * period when Rust thins frames (2 s in Performance mode, D-088). Charts lay
 * rows on this grid, so a thinned stream reads as fewer samples, not gaps.
 */
export function gridIntervalMs(
  status: Pick<LiveStatus, "interval_ms" | "frame_period_ms"> | null
): number {
  if (!status) return 1000;
  return Math.max(status.interval_ms, status.frame_period_ms);
}

export interface SeriesWindow {
  /** Oldest first, one slot per interval, `null` where nothing was measured. */
  values: (number | null)[];
  tEndMs: number;
  intervalMs: number;
}

/** What `visitWindow` needs from the host. */
export type WindowState = Pick<
  HostLive,
  "columns" | "lastTsMs" | "status" | "layouts"
>;

/** Receives a window's slots that have a value (`visitWindow`). */
export interface WindowSink {
  /** Slot `slot` (0 is the oldest) has `value`. Each slot once, oldest first. */
  put(slot: number, value: number): void;
}

/** Slots in a `windowMs` window on a grid of `intervalMs`: at least one. */
export function windowSlots(windowMs: number, intervalMs: number): number {
  return Math.max(1, Math.round(windowMs / intervalMs));
}

/**
 * Walks one series over the last `windowMs`, laid on a regular grid of the
 * current row spacing (`gridIntervalMs`) and ending at the newest row, and
 * hands `sink` every slot that has a value. Slot `k` sits at
 * `lastTsMs - (slots - 1 - k) * intervalMs`. Allocates nothing, so window
 * statistics can run it per key per tick.
 *
 * Two consecutive samples of the series are one run when the later one
 * comes no more than the earlier one's hold after it, as the host published
 * it (D-090); otherwise there is a gap between them. Inside a run the empty
 * slots between them are filled for display: with the later value for a
 * `mean` or `rate` series, which is the average over those seconds (a
 * collector sampling every 10 s with no window open), and with the straight
 * line between them for a gauge, the segment the chart draws anyway. These
 * fills are display values only: nothing writes them anywhere
 * (data-boundary.md).
 */
export function visitWindow(
  state: WindowState,
  key: string,
  windowMs: number,
  sink: WindowSink
): void {
  const cols = state.columns;
  const col = cols.column(key);
  if (state.lastTsMs === null || !col) return;
  const intervalMs = gridIntervalMs(state.status);
  const slots = windowSlots(windowMs, intervalMs);
  const tEndMs = state.lastTsMs;
  const kind = kindOf(state.layouts, key);
  const spanAverage = kind === "mean" || kind === "rate";
  // Slot of a row at `t`: `last - Math.round((tEndMs - t) / intervalMs)`.
  const last = slots - 1;
  const start = cols.firstAfter(tEndMs - windowMs);
  // The previous sample: its slot, time, row and value. Before the window
  // when the first one inside joins it, so its span reaches into the window.
  // Its hold is looked up only when the next sample is more than a slot on.
  let has = false;
  let pSlot = 0;
  let pTs = 0;
  let pRow = 0;
  let pV = 0;
  for (let i = start - 1; i >= 0; i--) {
    const v = col[cols.slot(i)] as number;
    if (Number.isNaN(v)) {
      if (tEndMs - windowMs - cols.tsAt(i) > cols.holdAt(i, key)) break;
      continue;
    }
    pTs = cols.tsAt(i);
    pSlot = last - Math.round((tEndMs - pTs) / intervalMs);
    pRow = i;
    pV = v;
    has = true;
    break;
  }
  // The previous sample's slot is handed over only once the next sample
  // lands in a later one: two rows rounding to one slot keep the later value.
  let pending = false;
  for (let i = start; i < cols.length; i++) {
    const v = col[cols.slot(i)] as number;
    if (Number.isNaN(v)) continue;
    const tsMs = cols.tsAt(i);
    const slot = last - Math.round((tEndMs - tsMs) / intervalMs);
    if (!(pending && slot === pSlot)) {
      if (pending) sink.put(pSlot, pV);
      if (has && slot - pSlot > 1 && tsMs - pTs <= cols.holdAt(pRow, key)) {
        const span = slot - pSlot;
        for (let k = Math.max(1, -pSlot); k < span && pSlot + k < slots; k++) {
          sink.put(pSlot + k, spanAverage ? v : pV + ((v - pV) * k) / span);
        }
      }
    }
    pending = slot >= 0 && slot < slots;
    has = true;
    pSlot = slot;
    pTs = tsMs;
    pRow = i;
    pV = v;
  }
  if (pending) sink.put(pSlot, pV);
}

/**
 * One series over the last `windowMs` on the `visitWindow` grid, `null`
 * where nothing was measured, so charts break there.
 */
export function seriesWindow(
  state: WindowState,
  key: string,
  windowMs: number
): SeriesWindow {
  const intervalMs = gridIntervalMs(state.status);
  const values: (number | null)[] = new Array(
    windowSlots(windowMs, intervalMs)
  ).fill(null);
  visitWindow(state, key, windowMs, {
    put: (slot, v) => {
      values[slot] = v;
    },
  });
  return { values, tEndMs: state.lastTsMs ?? 0, intervalMs };
}

/** `key`'s catalog kind from the newest layout that carries it. */
function kindOf(
  layouts: Record<number, LayoutInfo>,
  key: string
): MetricKind | undefined {
  const nos = Object.keys(layouts).map(Number);
  for (let i = nos.length - 1; i >= 0; i--) {
    const kind = layouts[nos[i] as number]?.kinds.get(key);
    if (kind) return kind;
  }
  return undefined;
}
