import { useRouteError } from "react-router";
import { Button } from "~/components/ui/button";

/**
 * Route-level error boundary (error-handling.md): show what broke and a
 * reload action. Logged with the path so it can be reproduced; there is no
 * remote error reporting.
 */
export function RouteError() {
  const error = useRouteError();
  const message = error instanceof Error ? error.message : String(error);
  console.error("[route] render error", {
    path: window.location.pathname,
    error,
  });
  return (
    <main className="flex min-h-svh flex-col items-start justify-center gap-3 bg-background p-8 text-foreground">
      <h1 className="font-[590] text-[15px]">This view failed to render</h1>
      <p className="figures max-w-[60ch] text-[12px] text-muted-foreground">
        {message}
      </p>
      <Button size="sm" onClick={() => window.location.reload()}>
        Reload
      </Button>
    </main>
  );
}
