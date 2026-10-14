import { formatBytes, formatPercent } from "@core/format";
import type { DockerSnapshot } from "@core/generated/bindings";
import { useUnits } from "~/hooks/use-units";
import { ProjectScope } from "./project-scope";
export function ContainerSummary({
  snapshot,
  project,
  onProject,
}: {
  snapshot: DockerSnapshot;
  project: string | null;
  onProject: (project: string | null) => void;
}) {
  const { bytes } = useUnits();
  const aggregate =
    project === null
      ? snapshot.aggregate
      : snapshot.project_aggregates.find(
          (row) => row.project === (project || null)
        )?.aggregate;
  const cpu = aggregate?.cpu_cores == null ? null : aggregate.cpu_cores * 100;
  const memory = aggregate?.working_set_bytes ?? null;
  const partial = aggregate?.partial ?? false;
  const projectNames = [
    ...new Set(snapshot.containers.map((row) => row.project ?? "")),
  ].sort();
  if (project !== null && !projectNames.includes(project))
    projectNames.push(project);
  return (
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div className="figures flex flex-wrap items-baseline gap-x-5 gap-y-1 text-[13px]">
        {project !== null && <span>{project || "Standalone"}</span>}
        <span>
          CPU {formatPercent(cpu)} (
          {cpu === null ? "—" : (cpu / 100).toFixed(1)} cores)
        </span>
        <span>Memory {formatBytes(memory, { units: bytes })}</span>
        <span>
          {aggregate?.running ?? 0} running · {aggregate?.total ?? 0} total
          {partial ? " · partial" : ""}
        </span>
      </div>
      <ProjectScope
        projects={projectNames}
        value={project}
        onChange={onProject}
      />
    </div>
  );
}
