/**
 * The layout record — the shell's restorable arrangement,
 * versioned and kept strictly separate from content.
 *
 * `restoreLayout` is field-wise by contract: an unknown field is ignored, a
 * missing or wrong-typed field takes its default, and a floor id that no longer
 * resolves is dropped. It never throws — a persisted layout can therefore never
 * be the reason the application fails to start, which is the failure mode a
 * persisted-layout feature classically introduces.
 */

export interface DockSizes {
  sidebar: number;
  rightDock: number;
}

export interface LayoutRecord {
  version: 1;
  activeFloorId: string;
  openFloorIds: readonly string[];
  activeSubsystem: string;
  activeFacet?: string;
  sidebarVisible: boolean;
  rightDockVisible: boolean;
  dockSizes: DockSizes;
}

/** The result of a field-wise restore: every field present, references resolved. */
export interface RestoredLayout {
  /** Undefined when the record named a floor id that is not currently known. */
  activeFloorId: string | undefined;
  /** Filtered to the currently known floor ids. */
  openFloorIds: string[];
  activeSubsystem: string | undefined;
  activeFacet: string | undefined;
  sidebarVisible: boolean;
  rightDockVisible: boolean;
  dockSizes: DockSizes;
}

export const DEFAULT_DOCK_SIZES: DockSizes = {
  sidebar: 240,
  rightDock: 280,
};

/** What the shell falls back to with no record, or an unreadable one. */
export const DEFAULT_RESTORED_LAYOUT: RestoredLayout = {
  activeFloorId: undefined,
  openFloorIds: [],
  activeSubsystem: undefined,
  activeFacet: undefined,
  sidebarVisible: true,
  rightDockVisible: false,
  dockSizes: DEFAULT_DOCK_SIZES,
};

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : null;
}

function str(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function bool(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function num(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

/**
 * Restore a layout record field-wise. `knownFloorIds`, when given, is the set of
 * floors that currently exist — ids outside it are dropped rather than restored.
 */
export function restoreLayout(raw: unknown, knownFloorIds?: readonly string[]): RestoredLayout {
  const record = asRecord(raw);
  if (!record) {
    return {
      ...DEFAULT_RESTORED_LAYOUT,
    };
  }

  const known = knownFloorIds ? new Set(knownFloorIds) : null;
  const resolves = (id: string) => !known || known.has(id);

  const activeFloorId = str(record.activeFloorId);
  const openFloorIds = Array.isArray(record.openFloorIds)
    ? record.openFloorIds.filter((id): id is string => typeof id === "string" && resolves(id))
    : [];

  const sizes = asRecord(record.dockSizes);

  return {
    activeFloorId: activeFloorId && resolves(activeFloorId) ? activeFloorId : undefined,
    openFloorIds,
    activeSubsystem: str(record.activeSubsystem),
    activeFacet: str(record.activeFacet),
    sidebarVisible: bool(record.sidebarVisible, DEFAULT_RESTORED_LAYOUT.sidebarVisible),
    rightDockVisible: bool(record.rightDockVisible, DEFAULT_RESTORED_LAYOUT.rightDockVisible),
    dockSizes: {
      sidebar: num(sizes?.sidebar, DEFAULT_DOCK_SIZES.sidebar),
      rightDock: num(sizes?.rightDock, DEFAULT_DOCK_SIZES.rightDock),
    },
  };
}

/** Serialize the current arrangement into a v1 record for the settings store. */
export function toLayoutRecord(state: {
  activeFloorId: string;
  openFloorIds: readonly string[];
  activeSubsystem: string;
  activeFacet?: string;
  sidebarVisible: boolean;
  rightDockVisible: boolean;
  dockSizes?: DockSizes;
}): LayoutRecord {
  return {
    version: 1,
    activeFloorId: state.activeFloorId,
    openFloorIds: [
      ...state.openFloorIds,
    ],
    activeSubsystem: state.activeSubsystem,
    activeFacet: state.activeFacet,
    sidebarVisible: state.sidebarVisible,
    rightDockVisible: state.rightDockVisible,
    dockSizes: state.dockSizes ?? DEFAULT_DOCK_SIZES,
  };
}
