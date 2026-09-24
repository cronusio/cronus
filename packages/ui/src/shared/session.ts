/**
 * The session domain — per-projection request status, derived.
 *
 * The truth is the projection stores; this is an index over their kinds, so a
 * surface composing several projections can read one aggregate "loading" or
 * "partially unavailable" state instead of each projection re-deriving it. It
 * carries no data, only status.
 */

import type { Projection } from "./projection";
import { createStore, type Store } from "./store";

export type RequestStatus = "idle" | "pending" | "failed";

/** Map a projection's four-state kind onto its request status. */
export function statusOf(projection: Projection<unknown>): RequestStatus {
  switch (projection.kind) {
    case "pending":
      return "pending";
    case "unavailable":
      return "failed";
    default:
      // unrequested and loaded are both "not in flight, not failed"
      return "idle";
  }
}

export interface SessionState {
  /** Request status per projection id. An absent id reads as "idle". */
  readonly status: Readonly<Record<string, RequestStatus>>;
}

export type SessionAction =
  | {
      type: "observe";
      id: string;
      projection: Projection<unknown>;
    }
  | {
      type: "forget";
      id: string;
    };

function reduce(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case "observe": {
      const next = statusOf(action.projection);
      if (state.status[action.id] === next) {
        return state;
      }
      return {
        status: {
          ...state.status,
          [action.id]: next,
        },
      };
    }
    case "forget": {
      if (!(action.id in state.status)) {
        return state;
      }
      const rest = {
        ...state.status,
      };
      delete rest[action.id];
      return {
        status: rest,
      };
    }
    default:
      return state;
  }
}

const INITIAL_SESSION_STATE: SessionState = {
  status: {},
};

export type SessionStore = Store<SessionState, SessionAction>;

export function createSessionStore(initial: SessionState = INITIAL_SESSION_STATE): SessionStore {
  return createStore(initial, reduce);
}

/** True when no observed projection is in flight or failed. */
export function allSettled(state: SessionState): boolean {
  return Object.values(state.status).every((s) => s === "idle");
}

/** Ids of projections whose last request failed. */
export function unavailableIds(state: SessionState): string[] {
  return Object.entries(state.status)
    .filter(([, s]) => s === "failed")
    .map(([id]) => id);
}
