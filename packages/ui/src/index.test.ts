import { describe, expect, it } from "vitest";
import * as ui from "./index";

// The package's public value surface, frozen. A deliberate public-API change
// updates this list in the same commit; anything else is a regression. The list
// was trimmed from 46 to 43 when the two earlier composer roots and the surface
// catalog they alone fed were retired, leaving exactly one exported application
// root (`BuildingShell`); the status rail then added `StatusBar` (44); the action
// registry's catalog-derivation seam added `actionsFromCatalog`/`resolveLabel` (46).
const PUBLIC_API = [
  "BuildingFrame",
  "BuildingShell",
  "CANONICAL_TOKENS",
  "CommandPalette",
  "DEFAULT_LOCALE",
  "DEFAULT_SCHEME_ID",
  "DashboardPanel",
  "FloorTabBar",
  "GlobalSettingsOverlay",
  "L3_FACETS",
  "MENU",
  "MechanismNav",
  "NAV_LAYERS",
  "OfficeViewPanel",
  "RightDock",
  "SIDEBAR_PRIMARY",
  "SIDEBAR_TABS",
  "SIDEBAR_UTILITY",
  "SelectionSurface",
  "StatusBar",
  "SubsystemSidebar",
  "SurfacePlaceholder",
  "SurfaceRouter",
  "actionsFromCatalog",
  "commandPaletteDelegate",
  "composeSidebar",
  "createActionRegistry",
  "createCoreClient",
  "hasMechanismNav",
  "isBound",
  "isCanonicalOrder",
  "isChildLayer",
  "isClosable",
  "isUnloadable",
  "registerScheme",
  "resolveLabel",
  "resolveScheme",
  "resolveTheme",
  "schemeCatalog",
  "settingsTier",
  "shouldLoad",
  "surfaceAttributes",
  "t",
  "themeAttributes",
  "translator",
  "visibleMenu",
];

describe("public API freeze", () => {
  it("exports exactly the frozen symbol set", () => {
    expect(Object.keys(ui).sort()).toEqual(PUBLIC_API);
  });

  it("declares exactly one application root", () => {
    const roots = Object.keys(ui).filter((name) => /^(App|Workbench|BuildingShell)$/.test(name));
    expect(roots).toEqual([
      "BuildingShell",
    ]);
  });
});
