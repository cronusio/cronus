import { describe, expect, it } from "vitest";
import {
  DEFAULT_DOCK_SIZES,
  DEFAULT_RESTORED_LAYOUT,
  type LayoutRecord,
  restoreLayout,
  toLayoutRecord,
} from "./layout-record";

const full: LayoutRecord = {
  version: 1,
  activeFloorId: "core",
  openFloorIds: [
    "home",
    "core",
  ],
  activeSubsystem: "kanban",
  activeFacet: "board",
  sidebarVisible: false,
  rightDockVisible: true,
  dockSizes: {
    sidebar: 300,
    rightDock: 320,
  },
};

describe("restoreLayout — field-wise, never throws", () => {
  it("a full v1 record restores every field", () => {
    expect(
      restoreLayout(full, [
        "home",
        "core",
      ]),
    ).toEqual({
      activeFloorId: "core",
      openFloorIds: [
        "home",
        "core",
      ],
      activeSubsystem: "kanban",
      activeFacet: "board",
      sidebarVisible: false,
      rightDockVisible: true,
      dockSizes: {
        sidebar: 300,
        rightDock: 320,
      },
    });
  });

  it("a truncated record — missing fields take their defaults", () => {
    const restored = restoreLayout({
      version: 1,
      activeSubsystem: "chat",
    });
    expect(restored).toEqual({
      ...DEFAULT_RESTORED_LAYOUT,
      activeSubsystem: "chat",
    });
    expect(restored.sidebarVisible).toBe(true);
    expect(restored.dockSizes).toEqual(DEFAULT_DOCK_SIZES);
  });

  it("an extended record — unknown fields are ignored, known ones still read", () => {
    const restored = restoreLayout({
      version: 2,
      activeSubsystem: "memory",
      sidebarVisible: false,
      somethingFromTheFuture: {
        nested: true,
      },
      dockSizes: {
        sidebar: 260,
        rightDock: 260,
        gutter: 8,
      },
    });
    expect(restored.activeSubsystem).toBe("memory");
    expect(restored.sidebarVisible).toBe(false);
    expect(restored.dockSizes).toEqual({
      sidebar: 260,
      rightDock: 260,
    });
    expect("somethingFromTheFuture" in restored).toBe(false);
  });

  it("a floor id that no longer resolves is dropped, not restored", () => {
    const restored = restoreLayout(
      {
        version: 1,
        activeFloorId: "ghost",
        openFloorIds: [
          "home",
          "ghost",
          "core",
        ],
      },
      [
        "home",
        "core",
      ],
    );
    expect(restored.activeFloorId).toBeUndefined();
    expect(restored.openFloorIds).toEqual([
      "home",
      "core",
    ]);
  });

  it("with no known-floor set, ids pass through unchecked", () => {
    const restored = restoreLayout({
      version: 1,
      activeFloorId: "anything",
    });
    expect(restored.activeFloorId).toBe("anything");
  });

  it("garbage input — null, a string, a number, wrong-typed fields — all yield defaults", () => {
    expect(restoreLayout(null)).toEqual(DEFAULT_RESTORED_LAYOUT);
    expect(restoreLayout("not a record")).toEqual(DEFAULT_RESTORED_LAYOUT);
    expect(restoreLayout(42)).toEqual(DEFAULT_RESTORED_LAYOUT);
    expect(restoreLayout(undefined)).toEqual(DEFAULT_RESTORED_LAYOUT);
    expect(
      restoreLayout({
        sidebarVisible: "yes",
        dockSizes: "wide",
        openFloorIds: "home",
      }),
    ).toEqual(DEFAULT_RESTORED_LAYOUT);
  });
});

describe("toLayoutRecord", () => {
  it("stamps version 1 and round-trips through restoreLayout", () => {
    const record = toLayoutRecord({
      activeFloorId: "core",
      openFloorIds: [
        "home",
        "core",
      ],
      activeSubsystem: "kanban",
      activeFacet: "board",
      sidebarVisible: false,
      rightDockVisible: true,
      dockSizes: {
        sidebar: 300,
        rightDock: 320,
      },
    });
    expect(record.version).toBe(1);
    expect(
      restoreLayout(record, [
        "home",
        "core",
      ]).activeSubsystem,
    ).toBe("kanban");
  });

  it("defaults the dock sizes when the caller omits them", () => {
    const record = toLayoutRecord({
      activeFloorId: "home",
      openFloorIds: [
        "home",
      ],
      activeSubsystem: "dashboard",
      sidebarVisible: true,
      rightDockVisible: false,
    });
    expect(record.dockSizes).toEqual(DEFAULT_DOCK_SIZES);
  });
});
