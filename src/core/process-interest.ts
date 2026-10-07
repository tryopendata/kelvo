/**
 * Process interest for one window and host (D-066). Each consumer (a card, a
 * page) asks for the rows it shows as a `ProcessView`; Rust keeps one view
 * per window and a new `set_process_interest` replaces it, so the window
 * sends the union of its consumers' views. The union is sent once per
 * microtask, so consumers that mount together cost one command, and again
 * whenever the subscription's `stream` changes (a resubscribe gets a new id,
 * and Rust drops interest tagged with the old one).
 */
import type { ProcessSort, ProcessView } from "@core/generated/bindings";

export type { ProcessSort, ProcessView };

/** The full table at every sample: the Processes page. */
export const FULL_TABLE: ProcessView = {
  limit: null,
  sort: [],
  period_ms: null,
};

/**
 * The union of several views: no limit if any has none, else the largest;
 * every sort key in first-seen order; the shortest period (`null`, every
 * sample, is shortest); network rates (D-081), GPU time (D-085) and
 * listening ports if any view shows them.
 * `null` when there are no views (no interest).
 */
export function unionViews(views: readonly ProcessView[]): ProcessView | null {
  const first = views[0];
  if (!first) return null;
  let limit: number | null = first.limit;
  let period: number | null = first.period_ms;
  const sort: ProcessSort[] = [];
  let network = false;
  let gpu = false;
  let ports = false;
  for (const v of views) {
    network ||= v.network === true;
    gpu ||= v.gpu === true;
    ports ||= v.ports === true;
    limit =
      limit === null || v.limit === null ? null : Math.max(limit, v.limit);
    period =
      period === null || v.period_ms === null
        ? null
        : Math.min(period, v.period_ms);
    // Empty means CPU (bindings: `ProcessView.sort`).
    for (const s of v.sort.length === 0 ? (["cpu"] as const) : v.sort) {
      if (!sort.includes(s)) sort.push(s);
    }
  }
  // A full table needs no sort keys.
  const view: ProcessView = {
    limit,
    sort: limit === null ? [] : sort,
    period_ms: period,
  };
  return {
    ...view,
    ...(network ? { network } : {}),
    ...(gpu ? { gpu } : {}),
    ...(ports ? { ports } : {}),
  };
}

function sameView(a: ProcessView | null, b: ProcessView | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.limit === b.limit &&
    a.period_ms === b.period_ms &&
    (a.network === true) === (b.network === true) &&
    (a.gpu === true) === (b.gpu === true) &&
    (a.ports === true) === (b.ports === true) &&
    a.sort.length === b.sort.length &&
    a.sort.every((s, i) => b.sort[i] === s)
  );
}

export type SendInterest = (view: ProcessView | null, stream: number) => void;

/**
 * The registry behind `useProcessInterest`. `send` gets the union (`null`
 * for no interest) and the current stream; nothing is sent before the first
 * stream is known, since Rust ties interest to it.
 */
export class ProcessInterest {
  private readonly views = new Map<symbol, ProcessView>();
  private stream: number | null = null;
  private sent: { view: ProcessView | null; stream: number } | null = null;
  private scheduled = false;

  constructor(private readonly send: SendInterest) {}

  /** Add or replace one consumer's view. */
  set(id: symbol, view: ProcessView): void {
    this.views.set(id, view);
    this.schedule();
  }

  remove(id: symbol): void {
    if (this.views.delete(id)) this.schedule();
  }

  /** The live subscription's id, `null` while (re)subscribing. */
  setStream(stream: number | null): void {
    if (this.stream === stream) return;
    this.stream = stream;
    this.schedule();
  }

  /** The union currently wanted, for tests and logging. */
  current(): ProcessView | null {
    return unionViews([...this.views.values()]);
  }

  private schedule(): void {
    if (this.scheduled) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      this.flush();
    });
  }

  private flush(): void {
    if (this.stream === null) return;
    const view = this.current();
    const sent = this.sent;
    if (sent && sent.stream === this.stream && sameView(sent.view, view)) {
      return;
    }
    // A new stream starts with no interest on the Rust side, so "none" need
    // not be said for it.
    if (view === null && (!sent || sent.stream !== this.stream)) {
      this.sent = { view, stream: this.stream };
      return;
    }
    this.sent = { view, stream: this.stream };
    this.send(view, this.stream);
  }
}
