import type { HostId, RemoteHostRecord } from "@core/generated/bindings";
import type { CommandResult, Transport } from "@core/transport";
import { MOCK_HOST_ID } from "./fixtures";
import { err } from "./protocol";

export type SpaceMethods = Pick<
  Transport,
  | "diskSpaceVolumes"
  | "diskSpaceStart"
  | "diskSpaceStatus"
  | "diskSpaceCancel"
  | "diskSpaceReport"
  | "diskSpaceSubtree"
  | "diskSpaceLargestFiles"
  | "diskSpaceReveal"
  | "diskSpacePath"
>;

/** The remote hosts the mock knows, each answering from its own peer. */
export type SpaceRemotes = ReadonlyMap<
  HostId,
  { record: RemoteHostRecord; peer: SpaceMethods }
>;

/**
 * Sends each space call to the host it names (D-117): this mock, or a remote
 * host's peer while it is connected (`remote_unreachable` otherwise, as the
 * session's call fails at once). Reveal in Finder is this Mac's only.
 */
export function routeDiskSpace(
  local: SpaceMethods,
  remotes: SpaceRemotes
): SpaceMethods {
  const at = (host: HostId): SpaceMethods | CommandResult<never> => {
    if (host === MOCK_HOST_ID) return local;
    const remote = remotes.get(host);
    if (!remote) return err({ kind: "unknown_host", host });
    if (remote.record.status.state !== "connected")
      return err({
        kind: "remote_unreachable",
        message: "the host is not connected",
      });
    return remote.peer;
  };
  const answer = async <T>(
    host: HostId,
    call: (to: SpaceMethods) => Promise<CommandResult<T>>
  ): Promise<CommandResult<T>> => {
    const to = at(host);
    return "status" in to ? to : call(to);
  };
  const me = MOCK_HOST_ID;
  return {
    diskSpaceVolumes: (host) => answer(host, (t) => t.diskSpaceVolumes(me)),
    diskSpaceStart: (host, volume, include) =>
      answer(host, (t) => t.diskSpaceStart(me, volume, include)),
    diskSpaceStatus: (host) => answer(host, (t) => t.diskSpaceStatus(me)),
    diskSpaceCancel: (host) => answer(host, (t) => t.diskSpaceCancel(me)),
    diskSpaceReport: (host) => answer(host, (t) => t.diskSpaceReport(me)),
    diskSpaceSubtree: (host, report, node) =>
      answer(host, (t) => t.diskSpaceSubtree(me, report, node)),
    diskSpaceLargestFiles: (host, report) =>
      answer(host, (t) => t.diskSpaceLargestFiles(me, report)),
    diskSpaceReveal: local.diskSpaceReveal,
    diskSpacePath: (host, report, target) =>
      answer(host, (t) => t.diskSpacePath(me, report, target)),
  };
}
