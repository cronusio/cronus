/**
 * The local cache of `capability_catalog`'s own answer — one `Invocable[]`
 * projection (the standard shape, applied to the invocable catalog) plus the
 * refresh that repopulates it.
 *
 * This surface's own catalog copy can go stale between an extension
 * activating/deactivating in the core and the next delivery. Dispatching an
 * id the local copy no longer recognises resolves to the registry's real
 * `Unknown` answer (`null`) — never a fabricated failure — and the
 * correct response here is to refresh the cache, not render an error. The
 * registry's own change-announcement mechanism is the primary refresh
 * trigger in production; `Unknown` is the backstop for the delivery window
 * between a change and its announcement, not the only path — this module
 * only builds the backstop.
 */

import type { CoreClient, Invocable, Invocation, Outcome } from "./bridge";
import { createProjectionStore, type ProjectionStore } from "./projection";

export type CatalogStore = ProjectionStore<Invocable[]>;

/** A fresh, unrequested catalog cache — call `refreshCatalog` to populate it. */
export function createCatalogStore(): CatalogStore {
  return createProjectionStore<Invocable[]>();
}

/** Re-fetch the catalog from the core and update `store` with the result. */
export async function refreshCatalog(client: CoreClient, store: CatalogStore): Promise<void> {
  store.dispatch({
    type: "request",
  });
  try {
    const data = await client.catalog();
    store.dispatch({
      type: "fulfill",
      data,
    });
  } catch (error) {
    store.dispatch({
      type: "fail",
      reason: error instanceof Error ? error.message : "catalog request failed",
    });
  }
}

/**
 * Dispatch one call through the core, refreshing `store` instead of
 * surfacing a failure when the core answers `Unknown` — the
 * desktop's own equivalent of the terminal UI's own ordinary-input
 * treatment for an unresolved line. `null` is returned either way: nothing
 * ran, so there is nothing for a caller to render as a result or an error.
 */
export async function dispatchThroughCatalog(
  client: CoreClient,
  store: CatalogStore,
  invocation: Invocation,
): Promise<Outcome | null> {
  const outcome = await client.invoke(invocation);
  if (outcome === null) {
    await refreshCatalog(client, store);
    return null;
  }
  return outcome;
}
