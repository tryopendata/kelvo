/** Presentation buffer only. Rust owns framing, stream identity and errors. */
export const LOG_MAX_LINES = 2_000;
export const LOG_MAX_BYTES = 2 * 1024 * 1024;
export const LOG_LINE_BYTES = 64 * 1024;

export interface DisplayLogLine {
  id: number;
  text: string;
  truncated: boolean;
  bytes: number;
}

export interface LogBuffer {
  lines: DisplayLogLine[];
  bytes: number;
  nextId: number;
  discarded: number;
}

export const emptyLogBuffer = (): LogBuffer => ({
  lines: [],
  bytes: 0,
  nextId: 0,
  discarded: 0,
});

/** UTF-8 caps also cover multibyte output; never interpret terminal markup. */
export function appendLogLines(
  buffer: LogBuffer,
  incoming: readonly string[]
): LogBuffer {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder("utf-8", { fatal: false });
  const lines = buffer.lines.slice();
  let { bytes, nextId, discarded } = buffer;
  for (const text of incoming) {
    const encoded = encoder.encode(text);
    const truncated = encoded.length > LOG_LINE_BYTES;
    let displayed = truncated
      ? decoder.decode(encoded.subarray(0, LOG_LINE_BYTES))
      : text;
    // A cut codepoint can decode to a three-byte replacement; keep the cap exact.
    while (encoder.encode(displayed).length > LOG_LINE_BYTES)
      displayed = displayed.slice(0, -1);
    const size = encoder.encode(displayed).length;
    lines.push({ id: nextId++, text: displayed, truncated, bytes: size });
    bytes += size;
    while (lines.length > LOG_MAX_LINES || bytes > LOG_MAX_BYTES) {
      const removed = lines.shift();
      if (removed) {
        bytes -= removed.bytes;
        discarded++;
      }
    }
  }
  return { lines, bytes, nextId, discarded };
}
