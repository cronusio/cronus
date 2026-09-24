/**
 * The capability-admission pass.
 *
 * Every method on the core seam must bind something that already exists — a core
 * capability another surface exercises, or a host-owned facility — and every
 * surface with no such counterpart must render an explicit placeholder, never
 * fabricated data. This suite pins both halves; the full enumeration lives in
 * the task's admission table.
 */

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createCoreClient, type InvokeFn } from "../shared/bridge";
import { SIDEBAR_PRIMARY, SIDEBAR_UTILITY } from "../shared/navigation";
import { BuildingShell } from "./building-shell";
import type { FloorTab } from "./floor-tab-bar";

const noInvoke = (() => Promise.reject(new Error("unused"))) as unknown as InvokeFn;

const HOME: FloorTab = {
  id: "home",
  name: "Home",
  kind: "home",
  state: "idle",
};

/**
 * Every seam method, with the counterpart that admits it. A method absent from
 * this map is one the rule would reject — the erosion it exists to stop.
 */
const ADMITTED: Record<string, string> = {
  version: "core capability (cronus_core::Capabilities::version; CLI/TUI bind it)",
  status: "core capability (cronus_core::Capabilities::status; CLI/TUI bind it)",
  catalog:
    "shared invocable registry (cronus_contract::Invocable; CLI/TUI project the same registry through their own catalogs)",
  invoke:
    "shared invocable dispatcher (cronus_contract::Outcome; CLI/TUI dispatch through the same Dispatcher)",
  settings: "host-owned facility (apps/desktop/tauri settings store)",
  subscribe: "core event-channel class (push edge; no channel emits yet)",
};

describe("capability admission — the seam", () => {
  it("exposes only methods with a named counterpart", () => {
    const seam = createCoreClient(noInvoke);
    expect(Object.keys(seam).sort()).toEqual(Object.keys(ADMITTED).sort());
  });

  it("has not grown a frontend-only method since version + status", () => {
    const seam = createCoreClient(noInvoke);
    for (const method of Object.keys(seam)) {
      expect(ADMITTED[method], `${method} needs a counterpart`).toBeTruthy();
    }
  });
});

describe("capability admission — the surfaces", () => {
  it("every sidebar surface is an explicit placeholder while nothing binds it", () => {
    for (const tab of [
      ...SIDEBAR_PRIMARY,
      ...SIDEBAR_UTILITY,
    ]) {
      const { unmount } = render(
        <BuildingShell
          floors={[
            HOME,
          ]}
          activeFloorId="home"
          activeSubsystem={tab}
        />,
      );
      const surface = screen.getByTestId(`surface-${tab}`);
      // office / dashboard have a panel, but with no loaded projection they are
      // still the placeholder — never a fabricated empty panel.
      expect(surface).toHaveAttribute("data-placeholder", "true");
      unmount();
    }
  });
});
