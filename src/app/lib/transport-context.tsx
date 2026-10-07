import type { Transport } from "@core/transport";
import { createContext, type ReactNode, useContext } from "react";

const TransportContext = createContext<Transport | null>(null);

export function TransportProvider({
  transport,
  children,
}: {
  transport: Transport;
  children: ReactNode;
}) {
  return (
    <TransportContext.Provider value={transport}>
      {children}
    </TransportContext.Provider>
  );
}

/** The window's transport (Tauri in the app, mock in the browser and tests). */
export function useTransport(): Transport {
  const transport = useContext(TransportContext);
  if (!transport) {
    throw new Error("useTransport must be used within a <TransportProvider>");
  }
  return transport;
}
