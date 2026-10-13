import { memo } from "react";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";

// Inventory snapshots update every second, but project choices usually stay the same.
export const ProjectScope = memo(
  function ProjectScope({
    projects,
    value,
    onChange,
  }: {
    projects: readonly string[];
    value: string | null;
    onChange: (value: string | null) => void;
  }) {
    return (
      <Select
        value={value === null ? "__all__" : value || "__standalone__"}
        onValueChange={(next) =>
          onChange(
            next === "__all__" ? null : next === "__standalone__" ? "" : next
          )
        }
      >
        <SelectTrigger aria-label="Project scope" size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="__all__">All projects</SelectItem>
          {projects.map((project) => (
            <SelectItem key={project} value={project || "__standalone__"}>
              {project || "Standalone"}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    );
  },
  (previous, next) =>
    previous.value === next.value &&
    previous.onChange === next.onChange &&
    previous.projects.length === next.projects.length &&
    previous.projects.every(
      (project, index) => project === next.projects[index]
    )
);
