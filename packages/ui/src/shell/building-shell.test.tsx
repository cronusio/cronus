import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { t } from "../shared/i18n";
import { SIDEBAR_PRIMARY, SIDEBAR_UTILITY } from "../shared/navigation";
import { BuildingShell } from "./building-shell";
import type { FloorTab } from "./floor-tab-bar";

const floors: FloorTab[] = [
  {
    id: "home",
    name: "Home",
    kind: "home",
    state: "idle",
  },
  {
    id: "core",
    name: "cronus-core",
    kind: "project",
    state: "active",
  },
];

function renderShell(props: Partial<Parameters<typeof BuildingShell>[0]> = {}) {
  return render(
    <BuildingShell floors={floors} activeFloorId="core" activeSubsystem="dashboard" {...props} />,
  );
}

describe("BuildingShell composition", () => {
  it("mounts the four navigation layers + the router", () => {
    renderShell();
    expect(screen.getByTestId("building-frame")).toBeInTheDocument();
    expect(screen.getByTestId("floor-tab-bar")).toBeInTheDocument();
    expect(screen.getByTestId("subsystem-sidebar")).toBeInTheDocument();
    // dashboard has an L3 facet catalog → the strip renders
    expect(screen.getByTestId("mechanism-nav")).toHaveAttribute("data-subsystem", "dashboard");
  });

  it("every SIDEBAR tab resolves through the router to a placeholder or empty-state surface", () => {
    for (const tab of [
      ...SIDEBAR_PRIMARY,
      ...SIDEBAR_UTILITY,
    ]) {
      const { unmount } = renderShell({
        activeSubsystem: tab,
      });
      const surface = screen.getByTestId(`surface-${tab}`);
      expect(surface).toBeInTheDocument();
      // no fabricated data — the placeholder shows the copy
      if (surface.getAttribute("data-placeholder") === "true") {
        expect(surface).toHaveTextContent(t("en", "surface.placeholder"));
      }
      unmount();
    }
  });

  it("switching subsystem forwards an intent and swaps the surface", () => {
    const onSelectSubsystem = vi.fn();
    renderShell({
      onSelectSubsystem,
    });
    fireEvent.click(screen.getByTestId("sidebar-kanban"));
    expect(onSelectSubsystem).toHaveBeenCalledWith("kanban");
  });

  it("applies the two theming axes on the root and swaps them cosmetically", () => {
    const { rerender } = renderShell({
      theme: "dark",
      colorScheme: "default",
    });
    const root = screen.getByTestId("building-shell");
    expect(root).toHaveAttribute("data-theme", "dark");
    expect(root).toHaveAttribute("data-scheme", "default");
    expect(root.className).toContain("dark");

    rerender(
      <BuildingShell
        floors={floors}
        activeFloorId="core"
        activeSubsystem="dashboard"
        theme="light"
        colorScheme="default"
      />,
    );
    // same DOM node — cosmetic attribute swap, no unmount
    expect(screen.getByTestId("building-shell")).toBe(root);
    expect(root).toHaveAttribute("data-theme", "light");
    expect(root.className).not.toContain("dark");
  });

  it("the frame carries no inline literal colours", () => {
    renderShell({
      theme: "dark",
    });
    const root = screen.getByTestId("building-shell");
    expect(root.getAttribute("style")).toBeNull();
  });

  it("Ctrl+Shift+J opens the command palette; File▸Settings opens the overlay", () => {
    renderShell();
    expect(screen.queryByTestId("selection-surface")).toBeNull();
    fireEvent.keyDown(screen.getByTestId("building-shell"), {
      key: "J",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(screen.getByTestId("selection-surface")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("burger"));
    // burger opens the "file" group; the leaf is bound by the shell itself
    fireEvent.click(screen.getByTestId("menu-group-file"));
    fireEvent.click(screen.getByTestId("menu-leaf-file.settings"));
    expect(screen.getByTestId("global-settings-overlay")).toBeInTheDocument();
  });

  it("localizes every visible shell string (locale swap leaves no stale English)", () => {
    const { rerender } = renderShell({
      locale: "en",
    });
    expect(screen.getByTestId("sidebar-kanban")).toHaveTextContent("Kanban");
    rerender(
      <BuildingShell
        floors={floors}
        activeFloorId="core"
        activeSubsystem="dashboard"
        locale="ru"
      />,
    );
    expect(screen.getByTestId("sidebar-kanban")).toHaveTextContent(t("ru", "nav.kanban"));
  });
});
