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
    const remote: RemoteHostRecord = {
      config: saved,
      status: {
        state: saved.enabled ? "connected" : "disabled",
        route: saved.lan ? "lan" : "tailscale",
        last_seen_ms: this.ring.now(),
        error: null,
        sync: "current",
        clock_offset_ms: 120000,
        cpu_pct: peerScenarios.includes("linux") ? 3 : 18,
        memory_pct: peerScenarios.includes("linux") ? 43 : 42,
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
    return ok({
      installed: true,
      compatible: true,
      host_id: `remote-${lan ?? tailscale}`,
      name: lan ?? tailscale ?? "Remote Mac",
      message: null,
    });
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
    const result = this.addRemote(config);
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
    stopSharing
  ) => {
    this.commands.record("remove_remote_host", id, deleteMirror, stopSharing);
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
    if (this.options.scenarios?.includes("linux-host"))
      this.addRemote(
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
      listHosts: this.listHosts,
      getHost: this.getHost,
      getCapabilities: this.getCapabilities,
      emitHostsChanged: this.emitHostsChanged,
    };
  }
}
