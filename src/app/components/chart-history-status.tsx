export function ChartHistoryStatus({
  loading,
  error,
}: {
  loading?: boolean | undefined;
  error?: boolean | undefined;
}) {
  if (!loading && !error) return null;
  return (
    <p role="status" className="text-[11px] text-muted-foreground">
      {error
        ? "Couldn't load chart history. Change the range to try again."
        : "Loading chart history…"}
    </p>
  );
}
