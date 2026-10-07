import { useEffect, useState } from "react";

/**
 * Wall-clock time that refreshes every `everyMs` (uptime labels, "every 60
 * s" in plan 4.3). A hidden webview throttles the timer, which is fine for a
 * label nobody can see.
 */
export function useNow(everyMs: number): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), everyMs);
    return () => clearInterval(id);
  }, [everyMs]);
  return now;
}
