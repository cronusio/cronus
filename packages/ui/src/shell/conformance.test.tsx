/**
 * Phase 24 spec-conformance sweep — one named test per touched invariant class.
 * Cross-cuts the per-component suites; the detail lives there, the contract here.
 */

import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import process from "node:process";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  isCanonicalOrder,
  L3_FACETS,
  SIDEBAR_PRIMARY,
  SIDEBAR_TABS,
  SIDEBAR_UTILITY,
} from "../shared/navigation";
import { resolveScheme, surfaceAttributes } from "../shared/theme";
import { CANONICAL_TOKENS } from "../shared/tokens";
import { createActionRegistry } from "./actions";
import { BuildingShell } from "./building-shell";
import type { FloorTab } from "./floor-tab-bar";
import { MENU, visibleMenu } from "./menu";

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

describe("Phase 24 · spec conformance", () => {
  it("the sidebar is two frozen runs in fixed canonical order", () => {
    expect(Object.isFrozen(SIDEBAR_PRIMARY)).toBe(true);
    expect(Object.isFrozen(SIDEBAR_UTILITY)).toBe(true);
    expect(SIDEBAR_TABS).toEqual([
      ...SIDEBAR_PRIMARY,
      ...SIDEBAR_UTILITY,
    ]);
    expect(isCanonicalOrder(SIDEBAR_TABS)).toBe(true);
    expect(
      isCanonicalOrder([
        ...SIDEBAR_UTILITY,
        ...SIDEBAR_PRIMARY,
      ]),
    ).toBe(false);
  });

  it("L0 carries File/Edit/View/Help, the palette and the file-tree dock", () => {
    expect(MENU.map((g) => g.id)).toEqual([
      "file",
      "edit",
      "view",
      "help",
    ]);
    render(<BuildingShell floors={floors} activeFloorId="core" />);
    // command palette reachable
    fireEvent.keyDown(screen.getByTestId("building-shell"), {
      key: "J",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(screen.getByTestId("selection-surface")).toBeInTheDocument();
    // file-tree dock toggle present on the frame
    expect(screen.getByTestId("toggle-files")).toBeInTheDocument();
  });

  it("the L3 facet catalog is per-subsystem and earned, not uniform", () => {
    expect(L3_FACETS.schedule).toEqual([
      "cron",
      "pulse",
    ]);
    expect(L3_FACETS.memory).toBeUndefined();
    render(<BuildingShell floors={floors} activeFloorId="core" activeSubsystem="schedule" />);
    expect(screen.getByTestId("mechanism-nav")).toHaveAttribute("data-subsystem", "schedule");
  });

  it("switching mode or scheme is a cosmetic attribute swap, never behavioural", () => {
    const a = surfaceAttributes("dark", "default", true);
    const b = surfaceAttributes("light", "default", true);
    expect(a["data-theme"]).not.toBe(b["data-theme"]);
    // an unknown scheme never yields a blank surface
    expect(resolveScheme("dark", "made-up", true).schemeId).toBe("default");
  });

  it("the craft lint rejects a literal visual value outside the token layer", () => {
    const dir = mkdtempSync(join(tmpdir(), "conf-"));
    const bad = join(dir, "bad.tsx");
    writeFileSync(bad, 'const x = <div style={{ color: "#123456" }} />;\n');
    const script = join(process.cwd(), "scripts", "craft-lint.mjs");
    let code = 0;
    try {
      execFileSync("node", [
        script,
        bad,
      ]);
    } catch (e) {
      code =
        (
          e as {
            status?: number;
          }
        ).status ?? 1;
    }
    expect(code).toBe(1);
    // and the real component tree passes it
    expect(() =>
      execFileSync("node", [
        script,
      ]),
    ).not.toThrow();
    // every canonical token is a real, non-empty name
    expect(CANONICAL_TOKENS.every((t) => t.startsWith("--") && t.length > 3)).toBe(true);
  });

  it("no dead controls: an unbound menu leaf is dropped, surfaces are placeholders", () => {
    const reg = createActionRegistry([
      {
        id: "file.open",
        label: {
          key: "menu.file.open",
        },
        run: () => {},
      },
      {
        id: "file.exit",
        label: {
          key: "menu.file.exit",
        },
        run: () => {},
        bound: false,
      },
    ]);
    const file = visibleMenu(reg).find((g) => g.id === "file");
    const ids = file?.leaves.filter((l) => l !== null).map((l) => l?.actionId);
    expect(ids).toContain("file.open");
    expect(ids).not.toContain("file.exit");

    render(<BuildingShell floors={floors} activeFloorId="core" activeSubsystem="wiki" />);
    const surface = screen.getByTestId("surface-wiki");
    expect(surface).toHaveAttribute("data-placeholder", "true");
    expect(surface).toHaveTextContent("This surface will be populated by the core.");
  });
});
