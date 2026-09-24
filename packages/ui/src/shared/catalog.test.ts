import { describe, expect, it, vi } from "vitest";
import { createCoreClient, type Invocable, type InvokeFn, type Outcome } from "./bridge";
import { createCatalogStore, dispatchThroughCatalog, refreshCatalog } from "./catalog";

const descriptor: Invocable = {
  id: "core:board.list",
  name: "List cards",
  summary: "List every card on the board",
  group: "board",
  locus: "Semantic",
  binders: [],
  stability: "Shipped",
};

describe("refreshCatalog", () => {
  it("moves the store from unrequested through pending to loaded", async () => {
    const invoke = vi.fn();
    const client = createCoreClient(invoke as InvokeFn);
    vi.spyOn(client, "catalog").mockResolvedValue([
      descriptor,
    ]);
    const store = createCatalogStore();
    expect(store.snapshot().kind).toBe("unrequested");

    const pending = refreshCatalog(client, store);
    expect(store.snapshot().kind).toBe("pending");

    await pending;
    expect(store.snapshot()).toEqual({
      kind: "loaded",
      data: [
        descriptor,
      ],
    });
  });

  it("moves the store to unavailable when the catalog request fails", async () => {
    const invoke = vi.fn();
    const client = createCoreClient(invoke as InvokeFn);
    vi.spyOn(client, "catalog").mockRejectedValue(new Error("core unreachable"));
    const store = createCatalogStore();

    await refreshCatalog(client, store);

    expect(store.snapshot()).toEqual({
      kind: "unavailable",
      reason: "core unreachable",
    });
  });
});

describe("dispatchThroughCatalog — Unknown refreshes the cache instead of rendering a failure", () => {
  it("a resolved outcome passes through untouched and never refreshes the catalog", async () => {
    const outcome: Outcome = {
      Value: "Empty",
    };
    const invoke = vi.fn();
    const client = createCoreClient(invoke as InvokeFn);
    const invokeSpy = vi.spyOn(client, "invoke").mockResolvedValue(outcome);
    const catalogSpy = vi.spyOn(client, "catalog");
    const store = createCatalogStore();

    const result = await dispatchThroughCatalog(client, store, {
      id: "core:board.list",
    });

    expect(result).toEqual(outcome);
    expect(invokeSpy).toHaveBeenCalledTimes(1);
    expect(catalogSpy).not.toHaveBeenCalled();
    expect(store.snapshot().kind).toBe("unrequested");
  });

  it("dispatching an id absent from a stale cache resolves to null, renders no error, and refreshes the catalog exactly once", async () => {
    const invoke = vi.fn();
    const client = createCoreClient(invoke as InvokeFn);
    vi.spyOn(client, "invoke").mockResolvedValue(null);
    const catalogSpy = vi.spyOn(client, "catalog").mockResolvedValue([
      descriptor,
    ]);
    const store = createCatalogStore();
    // A stale local copy: something was cached once already.
    await refreshCatalog(client, store);
    catalogSpy.mockClear();

    const result = await dispatchThroughCatalog(client, store, {
      id: "core:extension.retired-verb",
    });

    // The ordinary-input-not-a-failure property this task names: no thrown
    // error, no rejected promise, no Outcome shape a caller could mistake
    // for a rendered failure — just the same `null` `invoke()` itself uses
    // for Unknown.
    expect(result).toBeNull();
    expect(catalogSpy).toHaveBeenCalledTimes(1);
    expect(store.snapshot()).toEqual({
      kind: "loaded",
      data: [
        descriptor,
      ],
    });
  });
});
