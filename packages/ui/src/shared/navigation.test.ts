import { describe, expect, it } from "vitest";
import {
  composeSidebar,
  type Floor,
  hasMechanismNav,
  isCanonicalOrder,
  isChildLayer,
  isClosable,
  isUnloadable,
  L3_FACETS,
  NAV_LAYERS,
  SIDEBAR_PRIMARY,
  SIDEBAR_TABS,
  SIDEBAR_UTILITY,
  settingsTier,
  shouldLoad,
} from "./navigation";

const homeFloor: Floor = {
  id: "home",
  name: "Home",
  kind: "home",
  loaded: true,
  hasRunningTask: false,
};

const projectFloor: Floor = {
  id: "proj-1",
  name: "Project 1",
  kind: "project",
  loaded: false,
  hasRunningTask: false,
};

describe("navigation model", () => {
  it("exposes the canonical two-run sidebar order and rejects reordering", () => {
    expect(SIDEBAR_PRIMARY[0]).toBe("dashboard");
    expect(SIDEBAR_PRIMARY[SIDEBAR_PRIMARY.length - 1]).toBe("wiki");
    expect(SIDEBAR_UTILITY).toEqual([
      "channels",
      "security",
      "providers",
      "settings",
    ]);
    expect(SIDEBAR_TABS).toEqual([
      ...SIDEBAR_PRIMARY,
      ...SIDEBAR_UTILITY,
    ]);
    expect(isCanonicalOrder(SIDEBAR_TABS)).toBe(true);
    const reordered = [
      ...SIDEBAR_TABS,
    ].reverse();
    expect(isCanonicalOrder(reordered)).toBe(false);
  });

  it("freezes both runs — pins cannot mutate them", () => {
    expect(Object.isFrozen(SIDEBAR_PRIMARY)).toBe(true);
    expect(Object.isFrozen(SIDEBAR_UTILITY)).toBe(true);
    const { pinned, primary, utility } = composeSidebar([
      "kanban",
      "memory",
    ]);
    expect(pinned).toEqual([
      "kanban",
      "memory",
    ]);
    expect(primary).toBe(SIDEBAR_PRIMARY);
    expect(utility).toBe(SIDEBAR_UTILITY);
    expect(
      isCanonicalOrder([
        ...primary,
        ...utility,
      ]),
    ).toBe(true);
  });

  it("resolves the per-subsystem L3 facet catalog", () => {
    expect(L3_FACETS.schedule).toEqual([
      "cron",
      "pulse",
    ]);
    expect(L3_FACETS.inbox).toEqual([
      "messages",
      "poll-clarify",
    ]);
    expect(L3_FACETS.dashboard).toEqual([
      "agent-statistics",
      "token-usage",
    ]);
    expect(hasMechanismNav("schedule")).toBe(true);
    expect(hasMechanismNav("memory")).toBe(false);
    expect(hasMechanismNav("chat")).toBe(false);
  });

  it("enforces strict four-layer nesting", () => {
    expect(NAV_LAYERS).toEqual([
      "building",
      "floor",
      "subsystem",
      "mechanism",
    ]);
    expect(isChildLayer("building", "floor")).toBe(true);
    expect(isChildLayer("floor", "subsystem")).toBe(true);
    expect(isChildLayer("building", "subsystem")).toBe(false);
    expect(isChildLayer("subsystem", "floor")).toBe(false);
  });

  it("pins the home floor as non-closable and always loaded", () => {
    expect(isClosable(homeFloor)).toBe(false);
    expect(isClosable(projectFloor)).toBe(true);
    expect(shouldLoad(homeFloor, "proj-1")).toBe(true);
  });

  it("lazy-loads project floors only when active or running", () => {
    expect(shouldLoad(projectFloor, "home")).toBe(false);
    expect(shouldLoad(projectFloor, "proj-1")).toBe(true);
    const running = {
      ...projectFloor,
      hasRunningTask: true,
    };
    expect(shouldLoad(running, "home")).toBe(true);
  });

  it("marks inactive idle project floors unloadable but never home", () => {
    expect(isUnloadable(projectFloor, "home")).toBe(true);
    expect(isUnloadable(projectFloor, "proj-1")).toBe(false);
    expect(isUnloadable(homeFloor, "proj-1")).toBe(false);
  });

  it("routes settings keys to the correct tier", () => {
    expect(settingsTier("appearance")).toBe("global");
    expect(settingsTier("models")).toBe("global");
    expect(settingsTier("office-identity")).toBe("local");
    expect(settingsTier("git")).toBe("local");
  });
});
