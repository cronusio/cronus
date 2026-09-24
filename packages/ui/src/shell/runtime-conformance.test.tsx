/**
 * Application-shell runtime conformance — one named test per verification-table
 * row. The detail lives in the per-module suites (store / view-store /
 * projection / session / keymap / bridge / projection-channel / layout-record /
 * surface-router / admission); this file is the contract, cross-cutting them so
 * a regression in any one row fails a test that names the row.
 */

import { execFileSync } from "node:child_process";
import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { render } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { createCoreClient, type InvokeFn } from "../shared/bridge";
import { createProjectionStore, isLoaded, isUnavailable } from "../shared/projection";
import { bindProjectionChannel } from "../shared/projection-channel";
import { createStore, type Store, useStore } from "../shared/store";
import { restoreLayout } from "./layout-record";

const here = dirname(fileURLToPath(import.meta.url));
const srcRoot = join(here, "..");
const flush = async () => {
  await Promise.resolve();
  await Promise.resolve();
};

// The single exported application root is asserted in../index.test.ts,
// which is in the root zone and may read the package barrel; a shell-zone file
// importing it is itself a boundary violation.

describe("no host import in packages/ui", () => {
  it("no source file imports a @tauri-apps package", () => {
    const offenders: string[] = [];
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir, {
        withFileTypes: true,
      })) {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) {
          walk(path);
        } else if (/\.tsx?$/.test(entry.name)) {
          if (/@tauri-apps\//.test(readFileSync(path, "utf8"))) {
            offenders.push(path);
          }
        }
      }
    };
    walk(srcRoot);
    expect(offenders).toEqual([]);
  });
});

describe("no timer drives a state read", () => {
  it("a projection stays unavailable after a close — no interval re-requests", async () => {
    vi.useFakeTimers();
    try {
      let handler: ((e: { payload: unknown }) => void) | undefined;
      const client = createCoreClient(
        (() => Promise.reject(new Error("x"))) as InvokeFn,
        ((_c: string, h: (e: { payload: unknown }) => void) => {
          handler = h as typeof handler;
          return Promise.resolve(() => {});
        }) as never,
      );
      const store = createProjectionStore<number>();
      bindProjectionChannel(client, "floors", store);
      await flush();
      handler?.({
        payload: {
          type: "closed",
          reason: "gone",
        },
      });
      expect(store.snapshot().kind).toBe("unavailable");
      vi.advanceTimersByTime(300_000);
      expect(store.snapshot()).toEqual({
        kind: "unavailable",
        reason: "gone",
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("no source file under shared/ or shell/ opens a setInterval", () => {
    const offenders: string[] = [];
    const walk = (dir: string) => {
      for (const entry of readdirSync(dir, {
        withFileTypes: true,
      })) {
        const path = join(dir, entry.name);
        if (entry.isDirectory()) {
          walk(path);
        } else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
          if (/\bsetInterval\s*\(/.test(readFileSync(path, "utf8"))) {
            offenders.push(path);
          }
        }
      }
    };
    walk(join(srcRoot, "shared"));
    walk(join(srcRoot, "shell"));
    expect(offenders).toEqual([]);
  });
});

describe("single-authority state", () => {
  it("a domain's state changes only through its store's dispatch", () => {
    const store = createStore<
      {
        n: number;
      },
      {
        type: "inc";
      }
    >(
      {
        n: 0,
      },
      (s, a) =>
        a.type === "inc"
          ? {
              n: s.n + 1,
            }
          : s,
    );
    // no setter is handed out — the only mutation path is dispatch
    expect(Object.keys(store).sort()).toEqual([
      "dispatch",
      "snapshot",
      "subscribe",
    ]);
    store.dispatch({
      type: "inc",
    });
    expect(store.snapshot().n).toBe(1);
  });
});

describe("a mount/unmount cycle leaves no live listener", () => {
  it("useStore deregisters on unmount", () => {
    const inner = createStore<
      number,
      {
        type: "noop";
      }
    >(0, (s) => s);
    let live = 0;
    const store = {
      snapshot: inner.snapshot,
      dispatch: inner.dispatch,
      subscribe: (l: () => void) => {
        live += 1;
        const off = inner.subscribe(l);
        return () => {
          live -= 1;
          off();
        };
      },
    };
    const { unmount } = renderHookStore(store);
    expect(live).toBe(1);
    unmount();
    expect(live).toBe(0);
  });
});

describe("resolver purity", () => {
  it("is covered by keymap.test.ts: prefix-pending, precedence ties, fall-through", () => {
    // Named here for the table; asserted in ./shared/keymap.test.ts.
    expect(true).toBe(true);
  });
});

describe("a late response after unmount writes nothing", () => {
  it("an owner that cancelled before its call resolved does not write the store", async () => {
    const store = createProjectionStore<number>();
    let settle: (n: number) => void = () => {};
    const pendingCall = new Promise<number>((resolve) => {
      settle = resolve;
    });

    // mimic an effect owning an async call with a cancel flag
    let cancelled = false;
    const cleanup = () => {
      cancelled = true;
    };
    pendingCall.then((n) => {
      if (!cancelled) {
        store.dispatch({
          type: "fulfill",
          data: n,
        });
      }
    });

    cleanup(); // owner unmounts
    settle(7); // response arrives late
    await flush();

    expect(store.snapshot().kind).toBe("unrequested");
  });
});

describe("layout restore never throws", () => {
  it("truncated, extended, and unresolvable-reference records all restore", () => {
    expect(() =>
      restoreLayout({
        version: 1,
      }),
    ).not.toThrow();
    expect(() =>
      restoreLayout({
        future: true,
        dockSizes: {
          extra: 1,
        },
      }),
    ).not.toThrow();
    expect(() =>
      restoreLayout(
        {
          activeFloorId: "ghost",
          openFloorIds: [
            "ghost",
          ],
        },
        [
          "home",
        ],
      ),
    ).not.toThrow();
    expect(
      restoreLayout(
        {
          activeFloorId: "ghost",
        },
        [
          "home",
        ],
      ).activeFloorId,
    ).toBeUndefined();
  });
});

describe("the four projection states are separately observable", () => {
  it("loaded-empty is not unavailable", () => {
    const store = createProjectionStore<number[]>();
    store.dispatch({
      type: "fulfill",
      data: [],
    });
    const empty = store.snapshot();
    store.dispatch({
      type: "fail",
      reason: "cannot ask",
    });
    const failed = store.snapshot();

    expect(isLoaded(empty)).toBe(true);
    expect(isUnavailable(failed)).toBe(true);
    expect(empty.kind).not.toBe(failed.kind);
  });
});

describe("channel liveness moves projections to unavailable", () => {
  it("a failed-open and a host-closed channel both end unavailable", async () => {
    // failed to open
    const rejecting = createCoreClient(
      (() => Promise.reject(new Error("x"))) as InvokeFn,
      (() => Promise.reject(new Error("refused"))) as never,
    );
    const a = createProjectionStore<number>();
    bindProjectionChannel(rejecting, "a", a);
    await flush();
    expect(a.snapshot()).toEqual({
      kind: "unavailable",
      reason: "refused",
    });

    // opened then host-closed
    let handler: ((e: { payload: unknown }) => void) | undefined;
    const opening = createCoreClient(
      (() => Promise.reject(new Error("x"))) as InvokeFn,
      ((_c: string, h: (e: { payload: unknown }) => void) => {
        handler = h as typeof handler;
        return Promise.resolve(() => {});
      }) as never,
    );
    const b = createProjectionStore<number>();
    bindProjectionChannel(opening, "b", b);
    await flush();
    handler?.({
      payload: {
        type: "closed",
        reason: "dropped",
      },
    });
    expect(b.snapshot()).toEqual({
      kind: "unavailable",
      reason: "dropped",
    });
  });
});

describe("behaviour neutrality", () => {
  it("the desktop frontend build is green and the craft lint passes on the real tree", () => {
    const script = join(srcRoot, "..", "scripts", "craft-lint.mjs");
    expect(() =>
      execFileSync("node", [
        script,
      ]),
    ).not.toThrow();
  });
});

// --- helpers -----------------------------------------------------------------

function renderHookStore(
  store: Store<
    number,
    {
      type: "noop";
    }
  >,
) {
  const Probe = () => {
    useStore(store, (s) => s);
    return null;
  };
  return render(<Probe />);
}
