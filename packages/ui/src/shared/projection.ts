/**
 * A projection — the frontend's cache of one core-owned fact, plus the honest
 * status of the request that produced it.
 *
 * A projection store never invents a value. Its snapshot is exactly one of four
 * states, and "loaded but empty" is not the same state as "could not ask": the
 * distinction between *no offices* and *the core did not answer* is preserved all
 * the way to the render, because collapsing it is how a shell starts showing
 * fabricated data. There is deliberately no default-empty state.
 *
 * This module is the type and the store factory. Nothing fetches here — a core
 * event, delivered over the bridge subscription, drives `request` / `fulfill` /
 * `fail`; that wiring lives with the seam, not here.
 */

import { createStore, type Store } from "./store";

export type Projection<T> =
  | {
      kind: "unrequested";
    }
  | {
      kind: "pending";
    }
  | {
      kind: "loaded";
      data: T;
    }
  | {
      kind: "unavailable";
      reason: string;
    };

export type ProjectionAction<T> =
  | {
      type: "request";
    }
  | {
      type: "fulfill";
      data: T;
    }
  | {
      type: "fail";
      reason: string;
    }
  | {
      type: "reset";
    };

/** Narrow to the loaded state — the only one carrying data a surface may render. */
export function isLoaded<T>(p: Projection<T>): p is {
  kind: "loaded";
  data: T;
} {
  return p.kind === "loaded";
}

/** Narrow to the unavailable state — carries the reason, never data. */
export function isUnavailable<T>(p: Projection<T>): p is {
  kind: "unavailable";
  reason: string;
} {
  return p.kind === "unavailable";
}

export function projectionReducer<T>(
  state: Projection<T>,
  action: ProjectionAction<T>,
): Projection<T> {
  switch (action.type) {
    case "request":
      return state.kind === "pending"
        ? state
        : {
            kind: "pending",
          };
    case "fulfill":
      return {
        kind: "loaded",
        data: action.data,
      };
    case "fail":
      return {
        kind: "unavailable",
        reason: action.reason,
      };
    case "reset":
      return state.kind === "unrequested"
        ? state
        : {
            kind: "unrequested",
          };
    default:
      return state;
  }
}

export type ProjectionStore<T> = Store<Projection<T>, ProjectionAction<T>>;

/** One projection store per core-owned domain. Starts *unrequested*. */
export function createProjectionStore<T>(
  initial: Projection<T> = {
    kind: "unrequested",
  },
): ProjectionStore<T> {
  return createStore(initial, projectionReducer<T>);
}
