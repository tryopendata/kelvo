import { createMockTransport } from "@core/mock-transport";
import { unwrap } from "@core/transport";
import { expect, it } from "vitest";

it("requires volume acknowledgement, consumes confirmations once, and refreshes removed resources", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const selection = [
    {
      kind: "volume" as const,
      id: "old-database-data",
      action: "delete" as const,
    },
  ];
  const review = await unwrap(transport.dockerStoragePrepare(selection));
  expect(
    (await transport.dockerStorageExecute(review.token, false)).status
  ).toBe("error");
  const started = await unwrap(
    transport.dockerStorageExecute(review.token, true)
  );
  expect(
    (await transport.dockerStorageExecute(review.token, true)).status
  ).toBe("error");
  expect(started.finished_ms).toBeNull();
  const operation = await unwrap(transport.dockerStorageOperation(started.id));
  expect(operation.results).toMatchObject([
    { status: "succeeded", reclaimed_bytes: null },
  ]);
  const scan = await unwrap(transport.dockerStorageScan(true));
  expect(
    scan.snapshot?.resources.some((row) => row.id === "old-database-data")
  ).toBe(false);
  transport.dispose();
});

it("rejects AutoRemove Stop, expired reviews and generation changes", async () => {
  let now = Date.now();
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
    now: () => now,
  });
  expect(
    (
      await transport.dockerStoragePrepare([
        { kind: "container", id: "container-1", action: "stop" },
      ])
    ).status
  ).toBe("error");
  const selections = [
    { kind: "container" as const, id: "container-0", action: "stop" as const },
  ];
  const old = await unwrap(transport.dockerStoragePrepare(selections));
  now += 60_001;
  expect((await transport.dockerStorageExecute(old.token, false)).status).toBe(
    "error"
  );
  const review = await unwrap(transport.dockerStoragePrepare(selections));
  const snapshot = await unwrap(transport.dockerSnapshot());
  await transport.dockerConfigure({
    ...snapshot.settings,
    enabled: false,
  });
  expect(
    (await transport.dockerStorageExecute(review.token, false)).status
  ).toBe("error");
  transport.dispose();
});

it("does not scan idle engines until explicit Resume", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-idle"],
    historyRows: 1,
  });
  const before = await unwrap(transport.dockerStorageScan(false));
  expect(before.snapshot).toBeNull();
  expect(before.error).toMatch(/Resume/);
  await transport.dockerResume();
  expect(
    (await unwrap(transport.dockerStorageScan(false))).snapshot?.complete
  ).toBe(true);
  transport.dispose();
});

it("requires a fresh review after another operation completes", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  const first = await unwrap(
    transport.dockerStoragePrepare([
      { kind: "image", id: "sha256:old-image", action: "delete" },
    ])
  );
  const second = await unwrap(
    transport.dockerStoragePrepare([
      { kind: "volume", id: "old-database-data", action: "delete" },
    ])
  );
  const started = transport.dockerStorageExecute(first.token, false);
  const blocked = transport.dockerStorageExecute(second.token, true);
  expect((await blocked).status).toBe("error");
  await started;
  expect(
    (await transport.dockerStorageExecute(second.token, true)).status
  ).toBe("error");
  const refreshed = await unwrap(
    transport.dockerStoragePrepare([
      { kind: "volume", id: "old-database-data", action: "delete" },
    ])
  );
  expect(
    (await transport.dockerStorageExecute(refreshed.token, true)).status
  ).toBe("ok");
  expect(
    (await transport.dockerStorageExecute(refreshed.token, true)).status
  ).toBe("error");
  transport.dispose();
});

it("preserves typed command failures for missing resources, stale reviews and busy operations", async () => {
  const transport = createMockTransport({
    scenarios: ["docker-32"],
    historyRows: 1,
  });
  try {
    expect(
      await transport.dockerDetail("missing-engine", "missing-container")
    ).toMatchObject({ status: "error", error: { kind: "docker_not_found" } });
    expect(
      await transport.dockerStorageOperation("missing-operation")
    ).toMatchObject({ status: "error", error: { kind: "docker_not_found" } });
    expect(
      await transport.dockerStorageExecute("missing-review", false)
    ).toMatchObject({
      status: "error",
      error: { kind: "docker_snapshot_expired" },
    });
    const review = await unwrap(
      transport.dockerStoragePrepare([
        { kind: "image", id: "sha256:old-image", action: "delete" },
      ])
    );
    const running = transport.dockerStorageExecute(review.token, false);
    const blocked = transport.dockerStoragePrepare([
      { kind: "volume", id: "old-database-data", action: "delete" },
    ]);
    expect(await blocked).toMatchObject({
      status: "error",
      error: { kind: "docker_resource_limit" },
    });
    await running;
  } finally {
    transport.dispose();
  }
});
