/**
 * Surface router — maps the active subsystem tab to a surface.
 *
 * In this slice every subsystem resolves to an explicit placeholder (no dead controls:
 * a control renders real content only when bound to a shipped capability; until
 * then it is a named placeholder, never fabricated data). The two already-built
 * panels (Office, Dashboard) render real content only from a *loaded* projection;
 * *pending* and *unavailable* stay placeholders that say which they are, so a
 * dead channel is never mistaken for a stable value. As a capability ships, its
 * case moves from placeholder to panel.
 */

import type { JSX } from "react";
import { type Locale, translator } from "../shared/i18n";
import type { SidebarTab } from "../shared/navigation";
import type { Projection } from "../shared/projection";
import {
  DashboardPanel,
  type DashboardProjection,
  type OfficeProjection,
  OfficeViewPanel,
} from "../surfaces";

export interface SurfaceRouterProps {
  active: SidebarTab;
  /** Four-state projections for the two built panels; absent reads as unrequested. */
  office?: Projection<OfficeProjection>;
  dashboard?: Projection<DashboardProjection>;
  locale?: Locale;
}

/**
 * The explicit "not real content here" surface. Never shows fabricated
 * data. `state` distinguishes *unrequested* / *pending* / *unavailable* in the
 * DOM (`data-state`), and `reason` carries the unavailable cause (`data-reason`);
 * neither is emitted when absent, so an unbound surface is byte-identical to
 * before.
 */
export function SurfacePlaceholder({
  tab,
  locale = "en",
  state,
  reason,
}: {
  tab: SidebarTab;
  locale?: Locale;
  state?: "unrequested" | "pending" | "unavailable";
  reason?: string;
}) {
  const msg = translator(locale);
  return (
    <div
      data-testid={`surface-${tab}`}
      data-placeholder="true"
      data-state={state}
      data-reason={reason}
      className="flex flex-1 flex-col items-center justify-center gap-2 p-8 text-center"
    >
      <p className="text-sm text-text-secondary">{msg("surface.placeholder")}</p>
      <p className="text-xs text-text-muted">{msg("surface.placeholder.hint")}</p>
    </div>
  );
}

/** Render a projected surface: its panel only when loaded, an explicit placeholder otherwise. */
function projectedSurface<T>(
  tab: SidebarTab,
  projection: Projection<T> | undefined,
  render: (data: T) => JSX.Element,
  locale: Locale,
): JSX.Element {
  const p: Projection<T> = projection ?? {
    kind: "unrequested",
  };
  if (p.kind === "loaded") {
    return (
      <div data-testid={`surface-${tab}`} className="flex-1 overflow-y-auto p-4">
        {render(p.data)}
      </div>
    );
  }
  return (
    <SurfacePlaceholder
      tab={tab}
      locale={locale}
      state={p.kind}
      reason={p.kind === "unavailable" ? p.reason : undefined}
    />
  );
}

export function SurfaceRouter({
  active,
  office,
  dashboard,
  locale = "en",
}: SurfaceRouterProps): JSX.Element {
  if (active === "office") {
    return projectedSurface(
      active,
      office,
      (data) => <OfficeViewPanel projection={data} mode="graph" locale={locale} />,
      locale,
    );
  }
  if (active === "dashboard") {
    return projectedSurface(
      active,
      dashboard,
      (data) => <DashboardPanel projection={data} locale={locale} />,
      locale,
    );
  }
  return <SurfacePlaceholder tab={active} locale={locale} />;
}
