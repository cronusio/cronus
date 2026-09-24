import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { SIDEBAR_PRIMARY, SIDEBAR_UTILITY } from "../shared/navigation";
import { createActionRegistry } from "./actions";
import { BuildingFrame } from "./building-frame";
import { type FloorTab, FloorTabBar } from "./floor-tab-bar";
import { MechanismNav } from "./mechanism-nav";
import { SubsystemSidebar } from "./subsystem-sidebar";

const noop = () => {};

describe("L0 BuildingFrame + application menu", () => {
  const fullRegistry = createActionRegistry(
    [
      "file.open",
      "file.new-project",
      "file.settings",
      "file.close-window",
      "file.exit",
      "edit.undo",
      "edit.redo",
      "edit.cut",
      "edit.copy",
      "edit.paste",
      "edit.select-all",
      "edit.find",
      "edit.find-next",
      "edit.find-previous",
      "view.reload",
      "view.actual-size",
      "view.zoom-in",
      "view.zoom-out",
      "view.copy-url",
      "help.documentation",
      "help.check-updates",
      "help.troubleshooting",
      "help.support",
      "help.about",
    ].map((id) => ({
      id,
      label: {
        key: "menu.file" as const,
      },
      run: noop,
    })),
  );

  it("the burger toggles the menu bar", () => {
    const onOpenGroup = vi.fn();
    render(<BuildingFrame actions={fullRegistry} onOpenGroup={onOpenGroup} />);
    expect(screen.queryByTestId("menu-bar")).toBeNull();
    fireEvent.click(screen.getByTestId("burger"));
    expect(onOpenGroup).toHaveBeenCalledWith("file");
  });

  it("lists all four command groups and opens one group's submenu at a time", () => {
    const { rerender } = render(
      <BuildingFrame actions={fullRegistry} openGroup="file" onOpenGroup={noop} />,
    );
    for (const g of [
      "file",
      "edit",
      "view",
      "help",
    ]) {
      expect(screen.getByTestId(`menu-group-${g}`)).toBeInTheDocument();
    }
    expect(screen.getByTestId("menu-leaf-file.settings")).toHaveTextContent("Settings…");
    expect(screen.queryByTestId("menu-leaf-edit.find-previous")).toBeNull();

    rerender(<BuildingFrame actions={fullRegistry} openGroup="edit" onOpenGroup={noop} />);
    expect(screen.getByTestId("menu-leaf-edit.find-previous")).toBeInTheDocument();
    expect(screen.getByTestId("menu-leaf-edit.select-all")).toHaveTextContent("Select All");
  });

  it("an unbound leaf is absent — never a dead control", () => {
    const partial = createActionRegistry([
      {
        id: "file.open",
        label: {
          key: "menu.file.open",
        },
        run: noop,
      },
      {
        id: "file.settings",
        label: {
          key: "menu.file.settings",
        },
        run: noop,
        bound: false,
      },
    ]);
    render(<BuildingFrame actions={partial} openGroup="file" onOpenGroup={noop} />);
    expect(screen.getByTestId("menu-leaf-file.open")).toBeInTheDocument();
    expect(screen.queryByTestId("menu-leaf-file.settings")).toBeNull();
    expect(screen.queryByTestId("menu-leaf-file.exit")).toBeNull();
  });

  it("clicking a leaf runs its action and asks the caller to close", () => {
    const run = vi.fn();
    const onOpenGroup = vi.fn();
    const reg = createActionRegistry([
      {
        id: "file.open",
        label: {
          key: "menu.file.open",
        },
        run,
      },
    ]);
    render(<BuildingFrame actions={reg} openGroup="file" onOpenGroup={onOpenGroup} />);
    fireEvent.click(screen.getByTestId("menu-leaf-file.open"));
    expect(run).toHaveBeenCalledOnce();
    expect(onOpenGroup).toHaveBeenCalledWith(null);
  });
});

describe("L1 FloorTabBar", () => {
  const floors: FloorTab[] = [
    {
      id: "home",
      name: "Home",
      kind: "home",
      state: "idle",
    },
    {
      id: "p1",
      name: "cronus-core",
      kind: "project",
      state: "active",
    },
  ];

  it("the home floor is first, has no close/delete menu control", () => {
    render(<FloorTabBar floors={floors} activeFloorId="p1" />);
    expect(screen.getByTestId("floor-home")).toHaveAttribute("data-home", "true");
    expect(screen.queryByTestId("floor-menu-home")).toBeNull();
    expect(screen.getByTestId("floor-menu-p1")).toBeInTheDocument();
  });

  it("the status dot reflects the injected OfficeState, not a poll", () => {
    render(<FloorTabBar floors={floors} activeFloorId="p1" />);
    expect(screen.getByTestId("floor-state-p1")).toHaveAttribute("data-state", "active");
    // Home is pinned and carries a home glyph, not a lifecycle dot: it is the
    // building, not an office that can be running or hibernating.
    expect(screen.queryByTestId("floor-state-home")).toBeNull();
  });

  it("the + control and a full-bar drop both request floor creation", () => {
    const onCreateFloor = vi.fn();
    render(<FloorTabBar floors={floors} activeFloorId="home" onCreateFloor={onCreateFloor} />);
    fireEvent.click(screen.getByTestId("floor-add"));
    fireEvent.drop(screen.getByTestId("floor-tab-bar"));
    expect(onCreateFloor).toHaveBeenCalledTimes(2);
  });

  it("selecting a floor forwards an intent", () => {
    const onSelectFloor = vi.fn();
    render(<FloorTabBar floors={floors} activeFloorId="home" onSelectFloor={onSelectFloor} />);
    fireEvent.click(screen.getByTestId("floor-select-p1"));
    expect(onSelectFloor).toHaveBeenCalledWith("p1");
  });
});

describe("L2 SubsystemSidebar + expanded catalog", () => {
  it("renders the primary run and a visually-separated foot utility group", () => {
    render(<SubsystemSidebar active="dashboard" />);
    for (const tab of SIDEBAR_PRIMARY) {
      expect(screen.getByTestId(`sidebar-${tab}`)).toBeInTheDocument();
    }
    const utility = screen.getByTestId("sidebar-utility");
    // `settings` is pinned to the footer beside Help — a global entry, not one
    // of the floor's subsystems — so the foot run holds the rest.
    for (const tab of SIDEBAR_UTILITY.filter((t) => t !== "settings")) {
      expect(utility).toContainElement(screen.getByTestId(`sidebar-${tab}`));
    }
    expect(screen.getByTestId("sidebar-settings")).toBeInTheDocument();
    // 11 primary + 4 utility
    expect(SIDEBAR_PRIMARY).toHaveLength(11);
    expect(SIDEBAR_UTILITY).toHaveLength(4);
  });

  it("pins render above the primary run without joining either frozen array", () => {
    render(
      <SubsystemSidebar
        active="chat"
        pinned={[
          "kanban",
          "memory",
        ]}
      />,
    );
    expect(screen.getByTestId("sidebar-pins")).toBeInTheDocument();
    // the pinned tabs still appear once in the primary run too
    expect(screen.getAllByTestId("sidebar-kanban")).toHaveLength(2);
  });

  it("marks the active tab and forwards selection", () => {
    const onSelect = vi.fn();
    render(<SubsystemSidebar active="kanban" onSelect={onSelect} />);
    expect(screen.getByTestId("sidebar-kanban")).toHaveAttribute("aria-current", "page");
    fireEvent.click(screen.getByTestId("sidebar-inbox"));
    expect(onSelect).toHaveBeenCalledWith("inbox");
  });

  it("renders badge counts from injected signals", () => {
    render(
      <SubsystemSidebar
        active="chat"
        badges={{
          inbox: 3,
          chat: 2,
        }}
      />,
    );
    expect(screen.getByTestId("sidebar-badge-inbox")).toHaveTextContent("3");
    expect(screen.queryByTestId("sidebar-badge-memory")).toBeNull();
  });

  it("carries a run-control that forwards play/pause/stop", () => {
    const onRun = vi.fn();
    render(<SubsystemSidebar active="chat" runState="running" onRun={onRun} />);
    expect(screen.getByTestId("run-control")).toHaveAttribute("data-state", "running");
    fireEvent.click(screen.getByTestId("run-pause"));
    expect(onRun).toHaveBeenCalledWith("paused");
  });
});

describe("L3 MechanismNav", () => {
  it("renders the facet strip for a subsystem with facets", () => {
    render(<MechanismNav subsystem="schedule" />);
    const strip = screen.getByTestId("mechanism-nav");
    expect(strip).toHaveAttribute("data-subsystem", "schedule");
    expect(screen.getByTestId("facet-cron")).toBeInTheDocument();
    expect(screen.getByTestId("facet-pulse")).toBeInTheDocument();
  });

  it("renders nothing for a flat subsystem", () => {
    const { container } = render(<MechanismNav subsystem="memory" />);
    expect(container).toBeEmptyDOMElement();
  });

  it("forwards facet selection", () => {
    const onSelectFacet = vi.fn();
    render(<MechanismNav subsystem="inbox" onSelectFacet={onSelectFacet} />);
    fireEvent.click(screen.getByTestId("facet-poll-clarify"));
    expect(onSelectFacet).toHaveBeenCalledWith("poll-clarify");
  });
});
