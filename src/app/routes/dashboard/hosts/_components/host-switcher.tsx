import { memo } from "react";
import { useLocation, useNavigate } from "react-router";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "~/components/ui/select";
import { useHostSelection } from "~/lib/host-selection";

// Host selection follows its own context; metric-only sidebar updates do not
// need to rebuild the closed selection menu on every sample.
export const HostSwitcher = memo(function HostSwitcher() {
  const selection = useHostSelection();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  if (!selection) return null;
  return (
    <div className="px-1 pb-3">
      <Select
        value={selection.selected}
        onValueChange={(id) => {
          selection.select(id);
          if (
            pathname === "/dashboard/hosts" ||
            pathname === "/dashboard/settings"
          )
            navigate("/dashboard/overview");
        }}
      >
        <SelectTrigger
          aria-label="Selected machine"
          size="sm"
          className="w-full text-[12px]"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {selection.hosts.map((host) => (
            <SelectItem key={host.id} value={host.id}>
              {host.is_local ? "This Mac" : host.display_name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  );
});
