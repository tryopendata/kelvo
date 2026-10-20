import type {
  RemoteHostConfig,
  RemoteHostRecord,
} from "@core/generated/bindings";
import type { MockCommands } from "./commands";
import type { MockTransport, MockTransportOptions } from "./contracts";
import type { MockEmitters } from "./emitters";
import {
  hostRecord,
  MOCK_HOST_ID,
  type ScenarioFlags,
  type ScenarioName,
  scenarioFlags,
} from "./fixtures";
import { err, ok } from "./protocol";
import { isLinuxAlias, mockLinuxPreparation } from "./remote-linux";
import type { MockRing } from "./ring";
export class MockRemotes {
  readonly remotes = new Map<
    string,
    { record: RemoteHostRecord; peer: MockTransport; flags: ScenarioFlags }
  >();
  private trustedAliases = new Set<string>();
  private setups = new Map<string, () => void>();
  constructor(
    private readonly options: MockTransportOptions,
    private readonly ring: MockRing,
    private readonly commands: MockCommands,
    private readonly emitters: MockEmitters,
    private readonly createPeer: (
      options: MockTransportOptions
    ) => MockTransport
  ) {}
  /** A remote's host record: what its peer reports, under the saved name. */
  private readonly remoteRecord = (
    { flags }: { flags: ScenarioFlags },
    id: string,
    name: string
  ) => ({
    ...hostRecord(flags, this.ring.startMs),
    id,
    is_local: false,
    display_name: name,
  });
  readonly allHosts = () => [
    hostRecord(this.ring.flags, this.ring.startMs),
    ...[...this.remotes.values()].map((remote) =>
      this.remoteRecord(
        remote,
        remote.record.config.id,
        remote.record.config.name
      )
    ),
  ];
  /** `peerScenarios` shape the peer: a remote Mac without battery or fans by default. */
  readonly addRemote = (
    config: RemoteHostConfig,
    peerScenarios: readonly ScenarioName[] = ["no-battery", "no-fans"]
  ): RemoteHostRecord => {
    const saved = {
      ...config,
      id: config.id || `remote-${config.lan ?? config.tailscale}`,
    };
    const linux = peerScenarios.includes("linux");
    const remote: RemoteHostRecord = {
      config: saved,
      os: linux ? "linux" : "mac_os",
      status: {
        state: saved.enabled ? "connected" : "disabled",
        route: saved.lan ? "lan" : "tailscale",
        last_seen_ms: this.ring.now(),
        error: null,
        sync: "current",
        clock_offset_ms: 120000,
        cpu_pct: linux ? 3 : 18,
        memory_pct: linux ? 43 : 42,
        agent_version: "0.1.0",
        update_available: false,
        identity_changed: null,
      },
    };
    this.remotes.set(saved.id, {
      record: remote,
      flags: scenarioFlags(peerScenarios),
      peer: this.createPeer({
        ...this.options,
        scenarios: [...peerScenarios],
        now: () => this.ring.now() + 120000,
      }),
    });
    return remote;
  };
  readonly remoteSshConfigStatus: MockTransport["remoteSshConfigStatus"] =
    async () => {
      this.commands.record("remote_ssh_config_status");
      return ok(
        this.options.scenarios?.includes("ssh-no-config")
          ? "missing"
          : "present"
      );
    };
  readonly inspectRemoteSshHost: MockTransport["inspectRemoteSshHost"] = async (
    alias
  ) => {
    this.commands.record("inspect_remote_ssh_host", alias);
    return ok({
      token: alias,
      alias,
      hostname: alias,
      port: 22,
      fingerprints: ["256 SHA256:exampleMockHostFingerprint (ED25519)"],
    });
  };
  readonly acceptRemoteSshHost: MockTransport["acceptRemoteSshHost"] = async (
    token
  ) => {
    this.commands.record("accept_remote_ssh_host", token);
    this.trustedAliases.add(token);
    return ok(null);
  };
  readonly listRemoteHosts: MockTransport["listRemoteHosts"] = async () => {
    return [...this.remotes.values()].map((r) => r.record);
  };
  readonly prepareRemoteHost: MockTransport["prepareRemoteHost"] = async (
    lan,
    tailscale
  ) => {
    this.commands.record("prepare_remote_host", lan, tailscale);
    if (this.options.scenarios?.includes("ssh-auth"))
      return err({
        kind: "remote_auth",
        message: "Permission denied (publickey).",
      });
    if (!lan && !tailscale)
      return err({
        kind: "invalid_argument",
        message: "Enter at least one SSH alias",
      });
    if (
      this.options.scenarios?.includes("ssh-untrusted") &&
      !this.trustedAliases.has(lan ?? tailscale ?? "")
    )
      return err({
        kind: "host_key_unknown",
        message:
          "Host key verification failed: verify the SSH host fingerprint.",
      });
    const alias = lan ?? tailscale ?? "";
    if (isLinuxAlias(alias)) {
      const first = [...this.remotes.values()][0]?.record.config.name ?? null;
      const linux = mockLinuxPreparation(
        this.identityReset.has(alias) ? "linux" : alias,
        first
      );
      return "error" in linux ? err(linux.error) : ok(linux.ok);
    }
    return ok({
      kind: "mac",
      installed: true,
      compatible: true,
      host_id: `remote-${alias}`,
      name: alias || "Remote Mac",
      message: null,
      linux: null,
    });
  };
  /** Aliases whose agent was given a new identity; they prepare as a fresh host. */
  private identityReset = new Set<string>();
  readonly resetRemoteHostIdentity: MockTransport["resetRemoteHostIdentity"] =
    async (lan, tailscale) => {
      this.commands.record("reset_remote_host_identity", lan, tailscale);
      this.identityReset.add(lan ?? tailscale ?? "");
      return this.prepareRemoteHost(lan, tailscale);
    };
  readonly updateRemoteAgent: MockTransport["updateRemoteAgent"] = async (
    id
  ) => {
    this.commands.record("update_remote_agent", id);
    const remote = this.remotes.get(id);
    if (!remote) return err({ kind: "unknown_host", host: id });
    remote.record.status = {
      ...remote.record.status,
      agent_version: "0.1.0",
      update_available: false,
    };
    this.emitHostsChanged();
    return ok(null);
  };
  readonly repairRemoteHost: MockTransport["repairRemoteHost"] = async (id) => {
    this.commands.record("repair_remote_host", id);
    const remote = this.remotes.get(id);
    if (!remote) return err({ kind: "unknown_host", host: id });
    const { config, status, os } = remote.record;
    const newId = status.identity_changed ?? `${id}-repaired`;
    remote.record = {
      ...remote.record,
      config: {
        ...config,
        name: `${config.name} (before re-pair)`,
        enabled: false,
      },
      status: {
        ...remote.record.status,
        state: "disabled",
        error: null,
        identity_changed: null,
      },
    };
    const record = this.addRemote(
      {
        ...config,
        id: newId,
      },
      os === "linux" ? ["linux"] : undefined
    );
    this.emitHostsChanged();
    return ok(record);
  };
  readonly cancelRemoteSetup: MockTransport["cancelRemoteSetup"] = async (
    setupId
  ) => {
    this.commands.record("cancel_remote_setup", setupId);
    this.setups.get(setupId)?.();
    return ok(null);
  };
  readonly addRemoteHost: MockTransport["addRemoteHost"] = async (
    config,
    install,
    showMenuBar,
    setupId
  ) => {
    this.commands.record(
      "add_remote_host",
      config,
      install,
      showMenuBar,
      setupId
    );
    if (this.options.remoteSetupDelayMs) {
      const cancelled = await new Promise<boolean>((resolve) => {
        const timer = setTimeout(() => {
          this.setups.delete(setupId);
          resolve(false);
        }, this.options.remoteSetupDelayMs);
        this.setups.set(setupId, () => {
          clearTimeout(timer);
          this.setups.delete(setupId);
          resolve(true);
        });
      });
      if (cancelled)
        return err({
          kind: "invalid_argument",
          message: "Setup cancelled. Remote settings were restored.",
        });
    }
    const alias = config.lan ?? config.tailscale ?? "";
    if (alias.includes("dup") && !this.identityReset.has(alias))
      return err({
        kind: "remote_identity",
        message:
          "This host reports the same identity as a host already in your host list",
      });
    const result = this.addRemote(
      config,
      isLinuxAlias(config.lan ?? config.tailscale ?? "") ? ["linux"] : undefined
    );
    this.emitHostsChanged();
    return ok(result);
  };
  readonly updateRemoteHost: MockTransport["updateRemoteHost"] = async (
    config
  ) => {
    const remote = this.remotes.get(config.id);
    if (!remote) return err({ kind: "unknown_host", host: config.id });
    remote.record = {
      ...remote.record,
      config,
      status: {
        ...remote.record.status,
        state: config.enabled ? "connected" : "disabled",
      },
    };
    this.emitHostsChanged();
    return ok(remote.record);
  };
  readonly removeRemoteHost: MockTransport["removeRemoteHost"] = async (
    id,
    deleteMirror,
    stopSharing,
    uninstallAgent,
    purgeAgent
  ) => {
    this.commands.record(
      "remove_remote_host",
      id,
      deleteMirror,
      stopSharing,
      uninstallAgent,
      purgeAgent
    );
    this.remotes.get(id)?.peer.dispose();
    this.remotes.delete(id);
    this.emitHostsChanged();
    return ok(null);
  };
  readonly retryRemoteHost: MockTransport["retryRemoteHost"] = async (id) => {
    const remote = this.remotes.get(id);
    if (!remote) return err({ kind: "unknown_host", host: id });
    remote.record.status.state = "connected";
    return ok(null);
  };
  readonly listHosts: MockTransport["listHosts"] = async () => {
    this.commands.record("list_hosts");
    return this.allHosts();
  };
  readonly getHost: MockTransport["getHost"] = async (host) => {
    const remote = this.remotes.get(host);
    if (remote)
      return ok(this.remoteRecord(remote, host, remote.record.config.name));
    return this.commands.hostCommand("get_host", host, [host], {}, () =>
      ok(hostRecord(this.ring.flags, this.ring.startMs))
    );
  };
  readonly getCapabilities: MockTransport["getCapabilities"] = async (host) => {
    const remote = this.remotes.get(host);
    if (remote) return remote.peer.getCapabilities(MOCK_HOST_ID);
    return this.commands.hostCommand("get_capabilities", host, [host], {}, () =>
      ok(this.ring.caps)
    );
  };
  readonly emitHostsChanged: MockTransport["emitHostsChanged"] = () => {
    const hosts = this.allHosts();
    this.emitters.hosts.emit({ hosts });
  };
  seedScenario() {
    if (this.options.scenarios?.includes("multi-host"))
      this.addRemote({
        id: "remote-studio",
        name: "Studio Mac",
        lan: "studio.local",
        tailscale: "studio-tailnet",
        enabled: true,
        mirror_budget_mb: 100,
      });
    if (this.options.scenarios?.includes("linux-host")) {
      const linux = this.addRemote(
        {
          id: "remote-linux",
          name: "linux-tailscale",
          lan: null,
          tailscale: "linux-tailscale",
          enabled: true,
          mirror_budget_mb: 100,
        },
        ["linux"]
      );
      if (this.options.scenarios.includes("linux-agent-update"))
        linux.status = {
          ...linux.status,
          agent_version: "0.0.9",
          update_available: true,
        };
      if (this.options.scenarios.includes("linux-identity-changed"))
        linux.status = {
          ...linux.status,
          state: "error",
          error:
            "remote host identity does not match: linux-tailscale identified 48670e8f-2bca-4868-b557-16b1f1289341, expected 91d73b19-5d36-43b5-87f1-884bd635378e",
          identity_changed: "48670e8f-2bca-4868-b557-16b1f1289341",
          cpu_pct: null,
          memory_pct: null,
        };
    }
  }
  dispose() {
    for (const remote of this.remotes.values()) remote.peer.dispose();
    for (const cancel of this.setups.values()) cancel();
  }

  get methods() {
    return {
      remoteSshConfigStatus: this.remoteSshConfigStatus,
      inspectRemoteSshHost: this.inspectRemoteSshHost,
      acceptRemoteSshHost: this.acceptRemoteSshHost,
      listRemoteHosts: this.listRemoteHosts,
      prepareRemoteHost: this.prepareRemoteHost,
      cancelRemoteSetup: this.cancelRemoteSetup,
      addRemoteHost: this.addRemoteHost,
      updateRemoteHost: this.updateRemoteHost,
      removeRemoteHost: this.removeRemoteHost,
      retryRemoteHost: this.retryRemoteHost,
      resetRemoteHostIdentity: this.resetRemoteHostIdentity,
      updateRemoteAgent: this.updateRemoteAgent,
      repairRemoteHost: this.repairRemoteHost,
      listHosts: this.listHosts,
      getHost: this.getHost,
      getCapabilities: this.getCapabilities,
      emitHostsChanged: this.emitHostsChanged,
    };
  }
}
