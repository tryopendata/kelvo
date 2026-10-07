import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { CardGrid, cardOrigin } from "./card-grid";
import { ModuleToggleList } from "./module-toggle-list";
import { SegmentedControl } from "./segmented-control";
import { Sidebar, type SidebarProps } from "./sidebar";
import { UnsupportedNotice } from "./unsupported-notice";

describe("SegmentedControl", () => {
  it("reports the picked option and ignores re-picking the current one", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <SegmentedControl
        ariaLabel="Sample interval"
        options={[
          { value: "500", label: "0.5s" },
          { value: "1000", label: "1s" },
        ]}
        value="1000"
        onChange={onChange}
      />
    );
    expect(screen.getByRole("radio", { name: "1s" })).toHaveAttribute(
      "aria-checked",
      "true"
    );
    await user.click(screen.getByRole("radio", { name: "1s" }));
    expect(onChange).not.toHaveBeenCalled();
    await user.click(screen.getByRole("radio", { name: "0.5s" }));
    expect(onChange).toHaveBeenCalledWith("500");
  });
});

const SIDEBAR: SidebarProps = {
  groups: [
    {
      id: "nav",
      entries: [
        {
          id: "overview",
          href: "/dashboard/overview",
          label: "Overview",
          icon: "overview",
        },
        {
          id: "timeline",
          href: "/dashboard/timeline",
          label: "Timeline",
          icon: "timeline",
        },
      ],
    },
    {
      id: "modules",
      entries: [
        {
          id: "cpu",
          href: "/dashboard/cpu",
          label: "CPU",
          icon: "cpu",
          value: "18%",
        },
        {
          id: "disk",
          href: "/dashboard/disk",
          label: "Disk",
          icon: "disk",
          value: "220M",
          disabled: true,
        },
      ],
    },
  ],
  active: "cpu",
  status: { intervalMs: 2000, paused: false, onBattery: true },
  version: "0.4.0",
};

describe("Sidebar", () => {
  it("marks the active entry and hides values of disabled modules", () => {
    render(<Sidebar {...SIDEBAR} />);
    expect(screen.getByRole("link", { name: /CPU/ })).toHaveAttribute(
      "aria-current",
      "page"
    );
    expect(screen.getByRole("link", { name: /Overview/ })).not.toHaveAttribute(
      "aria-current"
    );
    expect(screen.getByRole("link", { name: /Disk/ })).not.toHaveTextContent(
      "220M"
    );
    expect(screen.getByText(/on battery/)).toHaveTextContent(
      "Sampling every 2s · on battery"
    );
  });

  it("moves focus with arrow keys and navigates through the callback", async () => {
    const user = userEvent.setup();
    const onNavigate = vi.fn();
    render(<Sidebar {...SIDEBAR} onNavigate={onNavigate} />);
    screen.getByRole("link", { name: /Overview/ }).focus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("link", { name: /Timeline/ })).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("link", { name: /CPU/ })).toHaveFocus();
    await user.keyboard("{End}");
    expect(screen.getByRole("link", { name: /Disk/ })).toHaveFocus();
    await user.keyboard("{ArrowUp}{Enter}");
    expect(onNavigate).toHaveBeenCalledWith("/dashboard/cpu");
  });

  it("measures the selection pill when the route changes, not when values tick", () => {
    // jsdom has no layout; every read returns a new offset, so a measurement
    // shows up as a changed transform.
    let next = 0;
    const spy = vi
      .spyOn(HTMLElement.prototype, "offsetTop", "get")
      .mockImplementation(() => {
        next += 10;
        return next;
      });
    try {
      const { container, rerender } = render(<Sidebar {...SIDEBAR} />);
      const pill = container.querySelector<HTMLElement>("[data-sidebar-pill]");
      const placed = pill?.style.transform;
      expect(placed).toMatch(/^translateY\(\d+px\)$/);
      expect(pill?.style.opacity).toBe("1");

      const ticked = structuredClone(SIDEBAR);
      const cpu = ticked.groups[1]?.entries[0];
      if (cpu) cpu.value = "64%";
      rerender(<Sidebar {...ticked} />);
      expect(pill?.style.transform).toBe(placed);

      rerender(<Sidebar {...ticked} active="timeline" />);
      expect(pill?.style.transform).not.toBe(placed);
    } finally {
      spy.mockRestore();
    }
  });

  it("hides the selection pill when no entry is current", () => {
    const { container } = render(<Sidebar {...SIDEBAR} active="widgets" />);
    expect(
      container.querySelector<HTMLElement>("[data-sidebar-pill]")?.style.opacity
    ).toBe("0");
  });

  it("shows Paused instead of the interval", () => {
    render(
      <Sidebar
        {...SIDEBAR}
        status={{ intervalMs: 1000, paused: true, onBattery: false }}
      />
    );
    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.queryByText(/Sampling every/)).not.toBeInTheDocument();
  });
});

describe("ModuleToggleList", () => {
  it("toggles present modules and disables absent ones", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(
      <ModuleToggleList
        onToggle={onToggle}
        modules={[
          {
            id: "cpu",
            label: "CPU",
            description: "Load, clusters, per-core",
            accent: "cpu",
            enabled: true,
            available: true,
          },
          {
            id: "battery",
            label: "Battery",
            description: "Charge, health, cycles",
            accent: "battery",
            enabled: true,
            available: false,
          },
        ]}
      />
    );
    await user.click(screen.getByRole("switch", { name: "CPU" }));
    expect(onToggle).toHaveBeenCalledWith("cpu", false);

    const battery = screen.getByRole("switch", { name: "Battery" });
    expect(battery).toBeDisabled();
    expect(battery).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText("Not present on this Mac")).toBeInTheDocument();
  });
});

describe("UnsupportedNotice", () => {
  it("names the model and offers the sensor dump", async () => {
    const user = userEvent.setup();
    const onShareDump = vi.fn();
    render(<UnsupportedNotice modelId="Mac17,4" onShareDump={onShareDump} />);
    expect(screen.getByText("(Mac17,4)")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Share sensor dump" }));
    expect(onShareDump).toHaveBeenCalledOnce();
  });
});

describe("CardGrid", () => {
  it("rotates glow origins by index", () => {
    expect([0, 1, 2, 3, 4, 5].map(cardOrigin)).toEqual([
      "tl",
      "tr",
      "bl",
      "br",
      "tl",
      "tr",
    ]);
    render(
      <CardGrid items={["a", "b", "c"]} getKey={(s) => s}>
        {(item, origin) => <span>{`${item}:${origin}`}</span>}
      </CardGrid>
    );
    expect(screen.getByText("c:bl")).toBeInTheDocument();
  });
});
