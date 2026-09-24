/**
 * The four-layer building shell — composes L0 frame, L1 floor tabs, L2 sidebar,
 * L3 mechanism strip, the surface router, and the floating overlays (command
 * palette, right file-tree dock, global settings).
 *
 * Presentation only. All domain state (floors, office state, badge counts, the
 * file tree, recent offices) arrives as props from the hosting shell over the
 * IPC bridge; this component holds only view state (which floor/subsystem/facet
 * is active, which overlays are open) and forwards every mutation as an intent.
 *
 * Theming: the two axes (mode × scheme) are applied on the root via
 * `surfaceAttributes` — `data-theme` + `data-scheme` + the Tailwind `dark` class.
 * Switching either is a cosmetic attribute swap; the tree never unmounts.
 */

import { useMemo, useState } from "react";
import { type Locale, translator } from "../shared/i18n";
import {
  type BindingLayer,
  type ContextStack,
  eventToKeystroke,
  mergeKeymap,
  resolve,
} from "../shared/keymap";
import type { SidebarTab } from "../shared/navigation";
import type { Projection } from "../shared/projection";
import { useStore } from "../shared/store";
import { surfaceAttributes, type Theme } from "../shared/theme";
import type { DashboardProjection, OfficeProjection } from "../surfaces";
import {
  type ActionRegistry,
  createActionRegistry,
  resolveLabel,
  type ShellAction,
} from "./actions";
import { BuildingFrame } from "./building-frame";
import { CommandPalette } from "./command-palette";
import { type FloorTab, FloorTabBar } from "./floor-tab-bar";
import { GlobalSettingsOverlay } from "./global-settings-overlay";
import { restoreLayout } from "./layout-record";
import { MechanismNav } from "./mechanism-nav";
import { type FileNode, RightDock } from "./right-dock";
import { type BudgetMeter, StatusBar } from "./status-bar";
import { type RunControlState, SubsystemSidebar } from "./subsystem-sidebar";
import { SurfaceRouter } from "./surface-router";
import { createViewStore, INITIAL_VIEW_STATE } from "./view-store";

/**
 * The shell's context stack. The frame owns one context today — a populated
 * subset of the workspace / dock / panel vocabulary, declared as such; a real
 * focus path is future work. Actions and bindings resolve against this.
 */
const CONTEXT_STACK: ContextStack = [
  {
    id: "workspace",
    contexts: [
      "workspace",
    ],
  },
];

/**
 * The base binding layer. The palette shortcut lives here as a real binding
 * resolved through the keymap, not a hard-coded key comparison. Platform and
 * user layers merge over this; the user layer's persistence is the host's.
 */
const BASE_LAYER: BindingLayer = {
  name: "base",
  bindings: [
    {
      actionId: "view.command-palette",
      sequence: [
        "Ctrl+Shift+J",
      ],
    },
  ],
};
const KEYMAP = mergeKeymap([
  BASE_LAYER,
]);

export interface BuildingShellProps {
  // theming (mode × scheme)
  theme?: Theme;
  colorScheme?: string;
  systemPrefersDark?: boolean;
  onThemeChange?: (theme: Theme) => void;
  onColorSchemeChange?: (id: string) => void;
  schemes?: readonly {
    id: string;
    name: string;
  }[];
  // navigation state (caller-owned)
  floors: readonly FloorTab[];
  activeFloorId: string;
  activeSubsystem?: SidebarTab;
  onSelectFloor?: (id: string) => void;
  onCreateFloor?: () => void;
  onFloorMenu?: (id: string) => void;
  onSelectSubsystem?: (tab: SidebarTab) => void;
  pinned?: readonly SidebarTab[];
  badges?: Partial<Record<SidebarTab, number>>;
  floorName?: string;
  floorSlug?: string;
  /** Initials on the sidebar's manager avatar; derived from the name when absent. */
  floorInitials?: string;
  /** Counts that are a tally of what exists, not an alert (muted, no pill). */
  tallies?: Partial<Record<SidebarTab, string | number>>;
  /** Tabs carrying an unread marker with no number. */
  markers?: readonly SidebarTab[];
  /** Project run state, shown in the sidebar control and the status bar. */
  runState?: RunControlState;
  onRun?: (next: RunControlState) => void;
  /** Status-bar readouts. Each renders only when supplied — never a guess. */
  sessionBudget?: BudgetMeter;
  weeklyBudget?: BudgetMeter;
  gatesGreen?: boolean;
  memoryLabel?: string;
  onOpenProcessMonitor?: () => void;
  /** Secondary line beside the surface title in the content header. */
  surfaceSubtitle?: string;
  // L0 facilities
  actions?: readonly ShellAction[];
  fileTree?: readonly FileNode[];
  recentOffices?: readonly {
    id: string;
    name: string;
    hint?: string;
  }[];
  // surfaces (four-state projections; absent reads as unrequested)
  office?: Projection<OfficeProjection>;
  dashboard?: Projection<DashboardProjection>;
  /** A persisted layout record. Restored field-wise; absent = defaults. */
  initialLayout?: unknown;
  locale?: Locale;
}

export function BuildingShell({
  theme = "system",
  colorScheme = "default",
  systemPrefersDark = true,
  onThemeChange,
  onColorSchemeChange,
  schemes,
  floors,
  activeFloorId,
  activeSubsystem = "dashboard",
  onSelectFloor,
  onCreateFloor,
  onFloorMenu,
  onSelectSubsystem,
  pinned = [],
  badges = {},
  floorName,
  floorSlug,
  floorInitials,
  tallies = {},
  markers = [],
  runState = "stopped",
  onRun,
  sessionBudget,
  weeklyBudget,
  gatesGreen,
  memoryLabel,
  onOpenProcessMonitor,
  surfaceSubtitle,
  actions = [],
  fileTree = [],
  recentOffices = [],
  office,
  dashboard,
  initialLayout,
  locale = "en",
}: BuildingShellProps) {
  const msg = translator(locale);
  const surface = surfaceAttributes(theme, colorScheme, systemPrefersDark);

  // The view domain: one store per shell mount, read through selectors so
  // two regions needing the same fact take it from here, not a local copy.
  // Seeded field-wise from the persisted layout record — absent or
  // unreadable falls back to the documented initial state.
  const [view] = useState(() => {
    const restored = restoreLayout(initialLayout);
    return createViewStore({
      ...INITIAL_VIEW_STATE,
      sidebarOpen: restored.sidebarVisible,
      rightDockOpen: restored.rightDockVisible,
      activeFacet: restored.activeFacet,
    });
  });
  const openGroup = useStore(view, (s) => s.openGroup);
  const sidebarOpen = useStore(view, (s) => s.sidebarOpen);
  const rightDockOpen = useStore(view, (s) => s.rightDockOpen);
  const paletteOpen = useStore(view, (s) => s.paletteOpen);
  const settingsOpen = useStore(view, (s) => s.settingsOpen);
  const activeFacet = useStore(view, (s) => s.activeFacet);

  // The pending multi-keystroke prefix, held between key events. Empty
  // until a binding sequence is partially matched.
  const [pendingKeys, setPendingKeys] = useState<readonly string[]>([]);

  const registry: ActionRegistry = useMemo(() => {
    const openSettings: ShellAction = {
      id: "file.settings",
      label: {
        key: "menu.file.settings",
      },
      binding: "Ctrl ,",
      run: () =>
        view.dispatch({
          type: "setSettingsOpen",
          open: true,
        }),
    };
    const newProject: ShellAction = {
      id: "file.new-project",
      label: {
        key: "menu.file.new-project",
      },
      binding: "Ctrl N",
      run: () => onCreateFloor?.(),
    };
    return createActionRegistry([
      openSettings,
      newProject,
      ...actions,
    ]);
  }, [
    actions,
    onCreateFloor,
    view,
  ]);

  const paletteActions = registry.live(CONTEXT_STACK).map((a) => ({
    id: a.id,
    label: resolveLabel(msg, a.label),
    binding: a.binding,
    run: a.run,
  }));

  // Map a resolved binding's action id to the shell's own view intent, else the
  // registered command. `view.command-palette` is view state, not a capability.
  const runBinding = (actionId: string) => {
    if (actionId === "view.command-palette") {
      view.dispatch({
        type: "setPaletteOpen",
        open: true,
      });
      return;
    }
    registry.get(actionId)?.run();
  };

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: shell-level shortcut listener; the palette is also reachable from the sidebar search button
    <div
      data-testid="building-shell"
      data-theme={surface["data-theme"]}
      data-scheme={surface["data-scheme"]}
      className={`relative flex h-screen flex-col bg-surface-0 text-text-primary ${surface.className}`}
      onKeyDown={(e) => {
        const outcome = resolve(eventToKeystroke(e), CONTEXT_STACK, KEYMAP, pendingKeys);
        if (outcome.kind === "pending") {
          e.preventDefault();
          setPendingKeys(outcome.prefix);
          return;
        }
        if (pendingKeys.length > 0) {
          setPendingKeys([]);
        }
        if (outcome.kind === "action") {
          e.preventDefault();
          runBinding(outcome.binding.actionId);
        }
        // unbound: fall through — never preventDefault, or text input breaks
      }}
    >
      <BuildingFrame
        actions={registry}
        openGroup={openGroup}
        onOpenGroup={(group) =>
          view.dispatch({
            type: "openGroup",
            group,
          })
        }
        onToggleSidebar={() =>
          view.dispatch({
            type: "toggleSidebar",
          })
        }
        onToggleRightDock={() =>
          view.dispatch({
            type: "toggleRightDock",
          })
        }
        sidebarOpen={sidebarOpen}
        rightDockOpen={rightDockOpen}
        locale={locale}
      />

      <FloorTabBar
        floors={floors}
        activeFloorId={activeFloorId}
        onSelectFloor={onSelectFloor}
        onCreateFloor={onCreateFloor}
        onFloorMenu={onFloorMenu}
        locale={locale}
      />

      <div className="flex min-h-0 flex-1">
        {sidebarOpen ? (
          <SubsystemSidebar
            active={activeSubsystem}
            onSelect={(tab) => {
              view.dispatch({
                type: "setFacet",
                facet: undefined,
              });
              onSelectSubsystem?.(tab);
            }}
            pinned={pinned}
            badges={badges}
            tallies={tallies}
            markers={markers}
            floorName={floorName}
            floorSlug={floorSlug}
            floorInitials={floorInitials}
            onOpenSearch={() =>
              view.dispatch({
                type: "setPaletteOpen",
                open: true,
              })
            }
            onOpenSettings={() =>
              view.dispatch({
                type: "setSettingsOpen",
                open: true,
              })
            }
            runState={runState}
            onRun={onRun}
            locale={locale}
          />
        ) : null}

        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-13.25 flex-none items-center gap-2.5 border-border-subtle border-b px-4">
            <span className="font-semibold text-lg text-text-primary">
              {msg(`nav.${activeSubsystem}` as const)}
            </span>
            {surfaceSubtitle ? (
              <span className="text-sm text-text-muted">{surfaceSubtitle}</span>
            ) : null}
            <div className="flex-1" />
            <MechanismNav
              subsystem={activeSubsystem}
              activeFacet={activeFacet}
              onSelectFacet={(facet) =>
                view.dispatch({
                  type: "setFacet",
                  facet,
                })
              }
              locale={locale}
            />
          </div>
          <SurfaceRouter
            active={activeSubsystem}
            office={office}
            dashboard={dashboard}
            locale={locale}
          />
        </div>

        <RightDock open={rightDockOpen} floorName={floorName} tree={fileTree} locale={locale} />
      </div>

      <StatusBar
        floorName={floorName}
        runState={runState}
        session={sessionBudget}
        weekly={weeklyBudget}
        gatesGreen={gatesGreen}
        memoryLabel={memoryLabel}
        onOpenProcessMonitor={onOpenProcessMonitor}
        locale={locale}
      />

      <CommandPalette
        open={paletteOpen}
        onClose={() =>
          view.dispatch({
            type: "setPaletteOpen",
            open: false,
          })
        }
        recentOffices={recentOffices}
        onGoToOffice={(id) => {
          onSelectFloor?.(id);
          view.dispatch({
            type: "setPaletteOpen",
            open: false,
          });
        }}
        onGoToSubsystem={(tab) => {
          onSelectSubsystem?.(tab);
          view.dispatch({
            type: "setPaletteOpen",
            open: false,
          });
        }}
        onOpenSettings={() => {
          view.dispatch({
            type: "setSettingsOpen",
            open: true,
          });
          view.dispatch({
            type: "setPaletteOpen",
            open: false,
          });
        }}
        actions={paletteActions}
        locale={locale}
      />

      <GlobalSettingsOverlay
        open={settingsOpen}
        onClose={() =>
          view.dispatch({
            type: "setSettingsOpen",
            open: false,
          })
        }
        theme={theme}
        onThemeChange={onThemeChange}
        colorScheme={colorScheme}
        onColorSchemeChange={onColorSchemeChange}
        schemes={schemes}
        locale={locale}
      />
    </div>
  );
}
