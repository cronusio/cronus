/**
 * The reactive substrate — a hand-rolled subscribable store, no state library.
 *
 * The application shell holds no domain state: every store built here
 * caches a projection of core state, or the frame's own view state. What that
 * needs is a subscribable value with a single mutation path — not the selectors,
 * middleware, and devtools a state-management dependency brings. A dependency
 * whose main value is managing complex client state is also an invitation to
 * grow some, which this package must not.
 *
 * `createStore` is the single authority for one domain: `dispatch` is the
 * only way its state changes, `subscribe` hands back the deregister function,
 * and `snapshot` is referentially stable while the state is unchanged so
 * a `useSyncExternalStore` reader never re-renders on identity churn.
 *
 * `useStore` is the scoped subscription: a component re-renders only when
 * the slice its selector returns actually changes, compared with `Object.is` by
 * default or a caller-supplied equality.
 */

import { useRef, useSyncExternalStore } from "react";

/**
 * Folds an action into the next state. Returning the *same* reference signals
 * "no change" — the store then neither advances its snapshot nor notifies.
 */
export type Reducer<S, A> = (state: S, action: A) => S;

/** One state domain: a stable snapshot, a subscriber set, and the sole mutation path. */
export interface Store<S, A> {
  /** Current state. Referentially stable until a `dispatch` changes it. */
  snapshot(): S;
  /** Register a listener; the returned function deregisters it. */
  subscribe(listener: () => void): () => void;
  /** The only way state changes. A reducer returning the same reference is a no-op. */
  dispatch(action: A): void;
}

export function createStore<S, A>(initialState: S, reduce: Reducer<S, A>): Store<S, A> {
  let state = initialState;
  const listeners = new Set<() => void>();

  return {
    snapshot: () => state,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    dispatch: (action) => {
      const next = reduce(state, action);
      if (Object.is(next, state)) {
        return;
      }
      state = next;
      for (const listener of listeners) {
        listener();
      }
    },
  };
}

const strictEqual = <T>(a: T, b: T): boolean => Object.is(a, b);

/**
 * Subscribe to one slice of a store. Re-renders only when `selector`'s result
 * changes under `isEqual` (default `Object.is`). The selection is memoized
 * against the store's stable snapshot: an unrelated dispatch that leaves the
 * slice equal returns the previous reference, so `useSyncExternalStore` does not
 * re-render. The subscription is owned by the calling component and is
 * torn down on unmount.
 */
export function useStore<S, A, T>(
  store: Store<S, A>,
  selector: (state: S) => T,
  isEqual: (a: T, b: T) => boolean = strictEqual,
): T {
  const memo = useRef<{
    state: S;
    selection: T;
  } | null>(null);

  const getSelection = (): T => {
    const state = store.snapshot();
    const cached = memo.current;
    if (cached && Object.is(cached.state, state)) {
      return cached.selection;
    }
    const selection = selector(state);
    if (cached && isEqual(cached.selection, selection)) {
      // Slice unchanged: keep the old reference, advance the state pointer so the
      // fast path catches the next read without re-running the selector.
      cached.state = state;
      return cached.selection;
    }
    memo.current = {
      state,
      selection,
    };
    return selection;
  };

  return useSyncExternalStore(store.subscribe, getSelection, getSelection);
}
