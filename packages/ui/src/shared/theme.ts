/**
 * Theming — two orthogonal axes over the design-token contract.
 *
 *   mode   ∈ system | light | dark   — the OS-appearance axis. `system` follows
 *                                       the OS preference; explicit choices pass
 *                                       through. Realized as `data-theme`.
 *   scheme ∈ <built-in ids> ∪ <user>  — the visual-language axis: a named design
 *                                       identity (a full token package). Realized
 *                                       as `data-scheme`.
 *
 * The active look is `(mode) × (scheme)` — the CSS selector
 * `:root[data-scheme="<id>"][data-theme="<mode>"]` supplies the token values.
 * Changing either axis is cosmetic-only: it swaps root attributes, never
 * unmounts or mutates state. Components read tokens (via Tailwind utilities or
 * `var(--token)`); nothing reads `mode` or `scheme` directly except the resolver.
 */

import defaultManifest from "./schemes/default/manifest.json";

// ── Axis 1 · mode ───────────────────────────────────────────────────────────

/** The persisted mode choice. */
export type Theme = "system" | "light" | "dark";

/** What a mode choice renders as, once the OS preference is known. */
export type ResolvedTheme = "light" | "dark";

/** Resolve `system` against the OS preference; explicit choices pass through. */
export function resolveTheme(theme: Theme, systemPrefersDark: boolean): ResolvedTheme {
  if (theme === "system") {
    return systemPrefersDark ? "dark" : "light";
  }
  return theme;
}

/** Token attributes for a resolved mode, applied on the surface root. */
export function themeAttributes(resolved: ResolvedTheme): {
  "data-theme": ResolvedTheme;
  className: string;
} {
  return {
    "data-theme": resolved,
    className: resolved === "dark" ? "dark" : "",
  };
}

// ── Axis 2 · colour scheme (design identity) ────────────────────────────────

/** Provenance of a scheme package. */
export interface SchemeProvenance {
  kind: "bundled" | "local" | "repository" | "registry";
  reference: string;
  importedAt?: string;
}

/** A schema-validated scheme package manifest. */
export interface SchemeManifest {
  id: string;
  name: string;
  category: string;
  provenance: SchemeProvenance;
  fidelity: "verbatim" | "normalized" | "hybrid";
  files: {
    light: string;
    dark: string;
  };
}

/** The scheme id every install ships and every fallback lands on. */
export const DEFAULT_SCHEME_ID = "default";

/**
 * The scheme catalog — layered built-in < project < personal with id-stable
 * override. This slice ships only the built-in layer; project / personal
 * schemes register at runtime through `registerScheme`.
 */
const catalog = new Map<string, SchemeManifest>([
  [
    DEFAULT_SCHEME_ID,
    defaultManifest as SchemeManifest,
  ],
]);

/** Register (or id-stably override) a scheme in the catalog. */
export function registerScheme(manifest: SchemeManifest): void {
  catalog.set(manifest.id, manifest);
}

/** Manifests currently in the catalog, in insertion order. */
export function schemeCatalog(): readonly SchemeManifest[] {
  return [
    ...catalog.values(),
  ];
}

/** The outcome of resolving `(mode, scheme)` into the active surface attributes. */
export interface ResolvedSurface {
  /** Which light/dark variant renders. */
  resolvedMode: ResolvedTheme;
  /** The scheme id that resolved — may differ from the request on fallback. */
  schemeId: string;
  /** Set when the requested scheme id was unknown and `default` was substituted. */
  warning?: string;
  /** Set when even `default` was unresolvable (corrupt install); the bare
   *  `:root` safe token set renders and the surface stays legible, never blank. */
  integrityError?: string;
}

/**
 * Resolve the two theming axes into one surface descriptor. Pure — no DOM, no
 * side effects. `resolveScheme` never throws and never yields a blank surface:
 * an unknown scheme falls back to `default` with a warning; an unresolvable
 * `default` falls back to the safe token set with an integrity error.
 */
export function resolveScheme(
  theme: Theme,
  schemeId: string,
  systemPrefersDark: boolean,
): ResolvedSurface {
  const resolvedMode = resolveTheme(theme, systemPrefersDark);

  if (catalog.has(schemeId)) {
    return {
      resolvedMode,
      schemeId,
    };
  }

  if (catalog.has(DEFAULT_SCHEME_ID)) {
    return {
      resolvedMode,
      schemeId: DEFAULT_SCHEME_ID,
      warning: `unknown colour scheme "${schemeId}" — fell back to "${DEFAULT_SCHEME_ID}"`,
    };
  }

  return {
    resolvedMode,
    schemeId: DEFAULT_SCHEME_ID,
    integrityError: `colour scheme "${DEFAULT_SCHEME_ID}" is unresolvable — rendering the built-in safe token set`,
  };
}

/**
 * Root attributes for a resolved `(mode, scheme)` — applied on the shell root so
 * the `[data-scheme][data-theme]` token blocks and the Tailwind `dark` class take
 * effect. Kept distinct from `themeAttributes` (mode only) for callers that only
 * theme the mode axis.
 */
export function surfaceAttributes(
  theme: Theme,
  schemeId: string,
  systemPrefersDark: boolean,
): {
  "data-theme": ResolvedTheme;
  "data-scheme": string;
  className: string;
  resolved: ResolvedSurface;
} {
  const resolved = resolveScheme(theme, schemeId, systemPrefersDark);
  return {
    "data-theme": resolved.resolvedMode,
    "data-scheme": resolved.schemeId,
    className: resolved.resolvedMode === "dark" ? "dark" : "",
    resolved,
  };
}
