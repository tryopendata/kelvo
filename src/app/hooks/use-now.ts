import { useEffect, useRef, useState } from "react";
import { useHostClock } from "~/lib/host-clock";

/**
 * Wall-clock time that refreshes every `everyMs` (uptime labels, "every 60
 * s" in plan 4.3). A hidden webview throttles the timer, which is fine for a
 * label nobody can see.
 */
export function useNow(everyMs: number): number {
  const clock = useHostClock();
  const clockRef = useRef(clock);
  clockRef.current = clock;
  const [now, setNow] = useState(() => clock());
  useEffect(() => {
    const id = setInterval(() => setNow(clockRef.current()), everyMs);
    return () => clearInterval(id);
  }, [everyMs]);
  return now;
}
