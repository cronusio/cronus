import { describe, expect, it } from "vitest";
import { createViewStore, INITIAL_VIEW_STATE } from "./view-store";

describe("view store — the shell's view domain", () => {
  it("a fresh store starts at the documented initial view state", () => {
    expect(createViewStore().snapshot()).toEqual(INITIAL_VIEW_STATE);
    expect(createViewStore().snapshot()).toEqual({
      openGroup: null,
      sidebarOpen: true,
      rightDockOpen: false,
      paletteOpen: false,
      settingsOpen: false,
      activeFacet: undefined,
    });
  });

  it("toggles are pure flips", () => {
    const store = createViewStore();
    store.dispatch({
      type: "toggleSidebar",
    });
    expect(store.snapshot().sidebarOpen).toBe(false);
    store.dispatch({
      type: "toggleRightDock",
    });
    expect(store.snapshot().rightDockOpen).toBe(true);
  });

  it("a set-action to the value already held is a no-op: snapshot identity kept", () => {
    const store = createViewStore();
    const before = store.snapshot();
    store.dispatch({
      type: "setPaletteOpen",
      open: false,
    });
    expect(store.snapshot()).toBe(before);
    store.dispatch({
      type: "setPaletteOpen",
      open: true,
    });
    expect(store.snapshot().paletteOpen).toBe(true);
  });

  it("setFacet round-trips a value and clears back to undefined", () => {
    const store = createViewStore();
    store.dispatch({
      type: "setFacet",
      facet: "cron",
    });
    expect(store.snapshot().activeFacet).toBe("cron");
    store.dispatch({
      type: "setFacet",
      facet: undefined,
    });
    expect(store.snapshot().activeFacet).toBeUndefined();
  });

  it("one mutation is visible to every reader of the same store — single authority", () => {
    const store = createViewStore();
    // two independent readers, as two shell regions would each subscribe
    const menuBarReads = () => store.snapshot().sidebarOpen;
    const sidebarReads = () => store.snapshot().sidebarOpen;

    expect(menuBarReads()).toBe(true);
    expect(sidebarReads()).toBe(true);

    // the menu bar's "toggle sidebar" control
    store.dispatch({
      type: "toggleSidebar",
    });

    // the sidebar's own visibility check sees it without holding a copy
    expect(menuBarReads()).toBe(false);
    expect(sidebarReads()).toBe(false);
  });

  it("notifies subscribers on a real change and not on a no-op", () => {
    const store = createViewStore();
    let notifications = 0;
    store.subscribe(() => {
      notifications += 1;
    });
    store.dispatch({
      type: "setSettingsOpen",
      open: true,
    }); // change
    store.dispatch({
      type: "setSettingsOpen",
      open: true,
    }); // no-op
    expect(store.snapshot().settingsOpen).toBe(true);
    expect(notifications).toBe(1);
  });
});
