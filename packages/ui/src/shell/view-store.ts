/**
 * The view domain — the building shell's own view state: which overlays
 * are open, whether the docks are shown, which menu group and mechanism facet
 * are active. This is the state the frontend owns outright — not a projection of
 * core state, and not a caller-owned navigation intent (`activeFloorId` /
 * `activeSubsystem` stay props).
 *
 * One instance per shell mount, created in `BuildingShell` and never at module
 * scope (no listener registry outlives its components). Regions read it
 * through `useStore` selectors, so two that need the same fact take it from here
 * rather than each holding a copy. It is also exactly the state the layout
 * record serializes.
 */

import { createStore, type Store } from "../shared/store";
import type { MenuGroupId } from "./menu";

export interface ViewState {
  /** The menu-bar group whose dropdown is open, or null. */
  openGroup: MenuGroupId | null;
  /** Left subsystem sidebar visible. */
  sidebarOpen: boolean;
  /** Right file-tree dock visible. */
  rightDockOpen: boolean;
  /** Command palette open. */
  paletteOpen: boolean;
  /** Global settings overlay open. */
  settingsOpen: boolean;
  /** Active mechanism-strip facet for the current subsystem, or undefined. */
  activeFacet: string | undefined;
}

export type ViewAction =
  | {
      type: "openGroup";
      group: MenuGroupId | null;
    }
  | {
      type: "toggleSidebar";
    }
  | {
      type: "toggleRightDock";
    }
  | {
      type: "setPaletteOpen";
      open: boolean;
    }
  | {
      type: "setSettingsOpen";
      open: boolean;
    }
  | {
      type: "setFacet";
      facet: string | undefined;
    };

/** The view state a freshly mounted shell starts in. */
export const INITIAL_VIEW_STATE: ViewState = {
  openGroup: null,
  sidebarOpen: true,
  rightDockOpen: false,
  paletteOpen: false,
  settingsOpen: false,
  activeFacet: undefined,
};

function reduce(state: ViewState, action: ViewAction): ViewState {
  switch (action.type) {
    case "openGroup":
      return state.openGroup === action.group
        ? state
        : {
            ...state,
            openGroup: action.group,
          };
    case "toggleSidebar":
      return {
        ...state,
        sidebarOpen: !state.sidebarOpen,
      };
    case "toggleRightDock":
      return {
        ...state,
        rightDockOpen: !state.rightDockOpen,
      };
    case "setPaletteOpen":
      return state.paletteOpen === action.open
        ? state
        : {
            ...state,
            paletteOpen: action.open,
          };
    case "setSettingsOpen":
      return state.settingsOpen === action.open
        ? state
        : {
            ...state,
            settingsOpen: action.open,
          };
    case "setFacet":
      return state.activeFacet === action.facet
        ? state
        : {
            ...state,
            activeFacet: action.facet,
          };
    default:
      return state;
  }
}

export type ViewStore = Store<ViewState, ViewAction>;

/** Create the shell's view store. One per mount; the caller holds it in `useState`. */
export function createViewStore(initial: ViewState = INITIAL_VIEW_STATE): ViewStore {
  return createStore(initial, reduce);
}
