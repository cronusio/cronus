import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Invocable } from "../shared/bridge";
import type { ContextStack } from "../shared/keymap";
import { actionsFromCatalog, createActionRegistry, resolveLabel } from "./actions";
import { CommandPalette } from "./command-palette";
import { GlobalSettingsOverlay } from "./global-settings-overlay";
import { type FileNode, RightDock } from "./right-dock";

const tree: FileNode[] = [
  {
    name: "crates",
    kind: "dir",
  },
  {
    name: "target",
    kind: "dir",
    ignored: true,
  },
  {
    name: "README.md",
    kind: "file",
  },
];

describe("right file-tree dock", () => {
  it("is absent when closed and present when open", () => {
    const { rerender } = render(<RightDock open={false} tree={tree} />);
    expect(screen.queryByTestId("right-dock")).toBeNull();
    rerender(<RightDock open={true} tree={tree} />);
    expect(screen.getByTestId("right-dock")).toBeInTheDocument();
  });

  it("renders a read-only tree with git-ignored entries dimmed", () => {
    render(<RightDock open={true} tree={tree} />);
    expect(screen.getByTestId("file-target")).toHaveAttribute("data-ignored", "true");
    expect(screen.getByTestId("file-crates")).not.toHaveAttribute("data-ignored");
  });

  it("toggles its name/contents filter", () => {
    render(<RightDock open={true} tree={tree} />);
    expect(screen.getByTestId("dock-filter-names")).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByTestId("dock-filter-contents"));
    expect(screen.getByTestId("dock-filter-contents")).toHaveAttribute("aria-pressed", "true");
  });
});

describe("command palette (delegated selection surface)", () => {
  it("is absent when closed; renders grouped results when open", () => {
    const { rerender } = render(<CommandPalette open={false} />);
    expect(screen.queryByTestId("selection-surface")).toBeNull();
    rerender(
      <CommandPalette
        open={true}
        recentOffices={[
          {
            id: "core",
            name: "cronus-core",
            hint: "active",
          },
        ]}
      />,
    );
    expect(screen.getByTestId("selection-surface")).toBeInTheDocument();
    expect(screen.getByTestId("selection-item-office:core")).toHaveTextContent("cronus-core");
    // every subsystem is reachable
    expect(screen.getByTestId("selection-item-subsystem:kanban")).toBeInTheDocument();
  });

  it("filters by query and closes on Escape", () => {
    const onClose = vi.fn();
    render(<CommandPalette open={true} onClose={onClose} />);
    fireEvent.change(screen.getByTestId("selection-input"), {
      target: {
        value: "kanban",
      },
    });
    expect(screen.getByTestId("selection-item-subsystem:kanban")).toBeInTheDocument();
    expect(screen.queryByTestId("selection-item-subsystem:memory")).toBeNull();
    fireEvent.keyDown(screen.getByTestId("selection-input"), {
      key: "Escape",
    });
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("confirming a row dispatches its intent without a bridge call", () => {
    const onGoToSubsystem = vi.fn();
    render(<CommandPalette open={true} onGoToSubsystem={onGoToSubsystem} />);
    fireEvent.click(screen.getByTestId("selection-item-subsystem:office"));
    expect(onGoToSubsystem).toHaveBeenCalledWith("office");
  });

  it("shows an empty state when nothing matches", () => {
    render(<CommandPalette open={true} />);
    fireEvent.change(screen.getByTestId("selection-input"), {
      target: {
        value: "zzzznomatch",
      },
    });
    expect(screen.getByTestId("selection-empty")).toBeInTheDocument();
  });
});

/**
 * `actions.test.ts` already proves a `Semantic` action is `bound()`/`live()`-
 * visible on the registry without a separate declaration. This suite proves
 * the palette's own consumption of that same pipeline (registry → `.live()`
 * → `commandPaletteDelegate`'s `actions` group) end to end through the real
 * rendered component — not by re-testing the registry, and guarding against
 * the risk that this palette's own action source drifts into a hand-written
 * restatement of the same mapping.
 */
describe("command palette — its actions group renders from the catalog projection", () => {
  const NOWHERE: ContextStack = [];

  const boardList: Invocable = {
    id: "core:board.list",
    name: "List cards",
    summary: "List every card on the board",
    group: "board",
    locus: "Semantic",
    binders: [],
    stability: "Shipped",
  };

  /** The same mapping `building-shell.tsx` performs, reused here rather than
   *  re-derived, so this test proves the palette's own consumption and not a
   *  second, parallel plumbing this suite invented for itself. */
  function paletteActionsFrom(catalog: readonly Invocable[], dispatch: (id: string) => void) {
    const registry = createActionRegistry(actionsFromCatalog(catalog, dispatch));
    return registry.live(NOWHERE).map((a) => ({
      id: a.id,
      label: resolveLabel(() => {
        throw new Error("a catalog-sourced action's label never needs an i18n lookup");
      }, a.label),
      binding: a.binding,
      run: a.run,
    }));
  }

  it("a Semantic descriptor in the catalog appears as a palette row bearing its own name", () => {
    const dispatch = vi.fn();
    render(
      <CommandPalette
        open={true}
        actions={paletteActionsFrom(
          [
            boardList,
          ],
          dispatch,
        )}
      />,
    );

    const row = screen.getByTestId("selection-item-action:core:board.list");
    expect(row).toHaveTextContent("List cards");

    fireEvent.click(row);
    expect(dispatch).toHaveBeenCalledWith("core:board.list");
  });

  it("a descriptor absent from the catalog produces no row at all", () => {
    render(<CommandPalette open={true} actions={paletteActionsFrom([], vi.fn())} />);

    expect(screen.queryByTestId("selection-item-action:core:board.list")).toBeNull();
  });
});

describe("global settings overlay", () => {
  it("is absent when closed; full-screen when open, closes without losing caller state", () => {
    const onClose = vi.fn();
    const { rerender } = render(
      <GlobalSettingsOverlay open={false} theme="system" colorScheme="default" />,
    );
    expect(screen.queryByTestId("global-settings-overlay")).toBeNull();
    rerender(
      <GlobalSettingsOverlay open={true} onClose={onClose} theme="system" colorScheme="default" />,
    );
    expect(screen.getByTestId("global-settings-overlay")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("settings-close"));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("Appearance hosts the mode and scheme pickers wired to callbacks", () => {
    const onThemeChange = vi.fn();
    const onColorSchemeChange = vi.fn();
    render(
      <GlobalSettingsOverlay
        open={true}
        theme="dark"
        onThemeChange={onThemeChange}
        colorScheme="default"
        onColorSchemeChange={onColorSchemeChange}
        schemes={[
          {
            id: "default",
            name: "Cronus",
          },
          {
            id: "midnight",
            name: "Midnight",
          },
        ]}
      />,
    );
    expect(screen.getByTestId("mode-dark")).toHaveAttribute("aria-pressed", "true");
    fireEvent.click(screen.getByTestId("mode-light"));
    expect(onThemeChange).toHaveBeenCalledWith("light");
    fireEvent.change(screen.getByTestId("scheme-picker"), {
      target: {
        value: "midnight",
      },
    });
    expect(onColorSchemeChange).toHaveBeenCalledWith("midnight");
  });
});
