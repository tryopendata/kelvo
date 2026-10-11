import { createContext, useContext } from "react";

/** Local views retain native wall time; remote providers supply source-relative time. */
export const HostClockContext = createContext<() => number>(() => Date.now());
export const useHostClock = () => useContext(HostClockContext);
