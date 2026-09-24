import { describe, expect, it, vi } from "vitest";
import type { Invocable } from "../shared/bridge";
import type { ContextStack } from "../shared/keymap";
import {
  actionsFromCatalog,
  createActionRegistry,
  isBound,
  resolveLabel,
  type ShellAction,
} from "./actions";

const NOWHERE: ContextStack = [];

function semantic(id: string, name: string): Invocable {
  return {
    id,
    name,
    summary: `${name} — a real core capability`,
    group: id.split(":")[1]?.split(".")[0] ?? "test",
    locus: "Semantic",
    binders: [],
    stability: "Shipped",
  };
}

describe("resolveLabel — one of two channels, never both", () => {
  it("resolves a key label through the supplied msg function", () => {
    const msg = vi.fn().mockReturnValue("Settings…");
    expect(
      resolveLabel(msg, {
        key: "menu.file.settings",
      }),
    ).toBe("Settings…");
    expect(msg).toHaveBeenCalledWith("menu.file.settings");
  });

  it("returns a text label unchanged, never touching msg", () => {
    const msg = vi.fn();
    expect(
      resolveLabel(msg, {
        text: "List cards",
      }),
    ).toBe("List cards");
    expect(msg).not.toHaveBeenCalled();
  });
});

describe("actionsFromCatalog — the registry is not a second catalog", () => {
  it("a Semantic descriptor in the fed-in catalog is bound/live without being separately declared", () => {
    const descriptor = semantic("core:board.list", "List cards");
    const registry = createActionRegistry(
      actionsFromCatalog(
        [
          descriptor,
        ],
        vi.fn(),
      ),
    );

    expect(registry.has("core:board.list")).toBe(true);
    expect(isBound(registry, "core:board.list")).toBe(true);
    expect(registry.bound().map((a) => a.id)).toContain("core:board.list");
    expect(registry.live(NOWHERE).map((a) => a.id)).toContain("core:board.list");
  });

  it("an id absent from the fed-in catalog is not visible at all — no separate unbound entry", () => {
    const registry = createActionRegistry(actionsFromCatalog([], vi.fn()));

    expect(registry.has("core:board.list")).toBe(false);
    expect(registry.get("core:board.list")).toBeUndefined();
    expect(isBound(registry, "core:board.list")).toBe(false);
  });

  it("dropping a descriptor between two calls drops its action just as directly", () => {
    const descriptor = semantic("core:board.list", "List cards");
    const withIt = createActionRegistry(
      actionsFromCatalog(
        [
          descriptor,
        ],
        vi.fn(),
      ),
    );
    const withoutIt = createActionRegistry(actionsFromCatalog([], vi.fn()));

    expect(withIt.has("core:board.list")).toBe(true);
    expect(withoutIt.has("core:board.list")).toBe(false);
  });

  it("only Semantic descriptors become actions — ClientLocal, HostOnly, and Installation stay out", () => {
    const clientLocal: Invocable = {
      ...semantic("core:pane.focus-next", "Focus next"),
      locus: "ClientLocal",
    };
    const hostOnly: Invocable = {
      ...semantic("core:capability.version", "Version"),
      locus: {
        HostOnly: {
          reason: "shell-owned marshalling, not core logic",
        },
      },
    };
    const installation: Invocable = {
      ...semantic("core:status", "Status"),
      locus: "Installation",
    };

    const actions = actionsFromCatalog(
      [
        clientLocal,
        hostOnly,
        installation,
      ],
      vi.fn(),
    );

    expect(actions).toEqual([]);
  });

  it("each action's label is the descriptor's own name, as pre-resolved text", () => {
    const descriptor = semantic("core:board.list", "List cards");
    const [action] = actionsFromCatalog(
      [
        descriptor,
      ],
      vi.fn(),
    );

    expect(action).toBeDefined();
    expect((action as ShellAction).label).toEqual({
      text: "List cards",
    });
  });

  it("run() dispatches the invocable's own id, nothing else", () => {
    const dispatch = vi.fn();
    const [action] = actionsFromCatalog(
      [
        semantic("core:board.list", "List cards"),
      ],
      dispatch,
    );

    (action as ShellAction).run();

    expect(dispatch).toHaveBeenCalledOnce();
    expect(dispatch).toHaveBeenCalledWith("core:board.list");
  });
});
