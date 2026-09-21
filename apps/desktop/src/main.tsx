import {
  BuildingShell,
  createCoreClient,
  type ListenFn,
  type RunControlState,
  type SidebarTab,
  schemeCatalog,
  type Theme,
} from "@cronus/ui";
import "@cronus/ui/styles.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import {
  previewBadges,
  previewDashboard,
  previewFileTree,
  previewFloors,
  previewMarkers,
  previewRecentOffices,
  previewSessionBudget,
  previewTallies,
  previewWeeklyBudget,
} from "./preview-data";

// The UI package stays shell-agnostic; the desktop shell injects Tauri's invoke
// and event listener. `listen` is adapted to the bridge's ListenFn shape
// (channel + payload-only handler -> Promise<unlisten>).
const tauriListen: ListenFn = <T,>(channel: string, handler: (event: { payload: T }) => void) =>
  listen<T>(channel, (event) =>
    handler({
      payload: event.payload,
    }),
  );

const core = createCoreClient(invoke, tauriListen);

// The registered colour schemes, mapped to the picker's shape.
const SCHEMES = schemeCatalog().map((m) => ({
  id: m.id,
  name: m.name,
}));

interface Restored {
  theme: Theme;
  colorScheme: string;
  layout: unknown;
}

const FALLBACK: Restored = {
  theme: "system",
  colorScheme: "default",
  layout: null,
};

function Shell({ theme: t0, colorScheme: c0, layout }: Restored) {
  const [theme, setTheme] = useState<Theme>(t0);
  const [colorScheme, setColorScheme] = useState(c0);
  const [activeFloorId, setActiveFloorId] = useState("core");
  const [activeSubsystem, setActiveSubsystem] = useState<SidebarTab>("dashboard");
  const [runState, setRunState] = useState<RunControlState>("running");
  const [systemPrefersDark] = useState(
    () =>
      typeof window !== "undefined" && window.matchMedia("(prefers-color-scheme: dark)").matches,
  );

  useEffect(() => {
    // The bridge is live; surface the core status in the console so the wiring
    // is verifiable without a fake status bar.
    core
      .status()
      .then((s) => console.info("[cronus] core:", s))
      .catch(() => console.info("[cronus] core unavailable"));
  }, []);

  const floor = previewFloors.find((f) => f.id === activeFloorId);

  return (
    <BuildingShell
      theme={theme}
      colorScheme={colorScheme}
      systemPrefersDark={systemPrefersDark}
      onThemeChange={(next) => {
        setTheme(next);
        void core.settings.set({
          theme: next,
        });
      }}
      onColorSchemeChange={(next) => {
        setColorScheme(next);
        void core.settings.set({
          colorScheme: next,
        });
      }}
      schemes={SCHEMES}
      floors={previewFloors}
      activeFloorId={activeFloorId}
      onSelectFloor={setActiveFloorId}
      activeSubsystem={activeSubsystem}
      onSelectSubsystem={setActiveSubsystem}
      floorName={floor?.name}
      floorSlug={floor ? `cronusio/${floor.name}` : undefined}
      floorInitials="OS"
      badges={previewBadges}
      tallies={previewTallies}
      markers={previewMarkers}
      fileTree={previewFileTree}
      recentOffices={previewRecentOffices}
      dashboard={{
        kind: "loaded",
        data: previewDashboard,
      }}
      surfaceSubtitle={floor?.name}
      runState={runState}
      onRun={setRunState}
      sessionBudget={previewSessionBudget}
      weeklyBudget={previewWeeklyBudget}
      gatesGreen
      memoryLabel="645.3 MB"
      initialLayout={layout}
      locale="en"
    />
  );
}

function Root() {
  // Read the persisted shell settings once before mounting, so the theming axes
  // and the layout record are applied at first paint (AS-12). A read failure is
  // not fatal — the shell mounts on defaults.
  const [restored, setRestored] = useState<Restored | null>(null);

  useEffect(() => {
    let alive = true;
    core.settings
      .get()
      .then((s) => {
        if (alive) {
          setRestored({
            theme: s.theme as Theme,
            colorScheme: s.colorScheme,
            layout: s.layout,
          });
        }
      })
      .catch(() => {
        if (alive) {
          setRestored(FALLBACK);
        }
      });
    return () => {
      alive = false;
    };
  }, []);

  // Brief: the WebView paints its own background until settings resolve.
  if (!restored) {
    return null;
  }
  return <Shell {...restored} />;
}

const container = document.getElementById("root");
if (container) {
  createRoot(container).render(
    <StrictMode>
      <Root />
    </StrictMode>,
  );
}
