import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { createStore, type Store, useStore } from "./store";

interface CounterState {
  count: number;
  label: string;
}

type CounterAction =
  | {
      type: "inc";
    }
  | {
      type: "relabel";
      label: string;
    }
  | {
      type: "noop";
    };

const reduce = (state: CounterState, action: CounterAction): CounterState => {
  switch (action.type) {
    case "inc":
      return {
        ...state,
        count: state.count + 1,
      };
    case "relabel":
      return {
        ...state,
        label: action.label,
      };
    default:
      return state;
  }
};

const make = (): Store<CounterState, CounterAction> =>
  createStore<CounterState, CounterAction>(
    {
      count: 0,
      label: "a",
    },
    reduce,
  );

describe("createStore — the reactive substrate", () => {
  it("dispatch is the only mutation path and notifies every subscriber", () => {
    const store = make();
    const seen: number[] = [];
    store.subscribe(() => seen.push(store.snapshot().count));

    store.dispatch({
      type: "inc",
    });
    store.dispatch({
      type: "inc",
    });

    expect(store.snapshot().count).toBe(2);
    expect(seen).toEqual([
      1,
      2,
    ]);
  });

  it("subscribe returns a deregister function that stops notifications", () => {
    const store = make();
    const listener = vi.fn();
    const off = store.subscribe(listener);

    store.dispatch({
      type: "inc",
    });
    off();
    store.dispatch({
      type: "inc",
    });

    expect(listener).toHaveBeenCalledTimes(1);
    expect(store.snapshot().count).toBe(2);
  });

  it("a reducer returning the same state is a no-op: snapshot identity held, no notify", () => {
    const store = make();
    const before = store.snapshot();
    const listener = vi.fn();
    store.subscribe(listener);

    store.dispatch({
      type: "noop",
    });

    expect(store.snapshot()).toBe(before);
    expect(listener).not.toHaveBeenCalled();
  });
});

describe("useStore — scoped selector subscription", () => {
  it("re-renders only when the selected slice changes", () => {
    const store = make();
    let renders = 0;
    const { result } = renderHook(() => {
      renders += 1;
      return useStore(store, (s) => s.count);
    });

    expect(result.current).toBe(0);
    expect(renders).toBe(1);

    act(() =>
      store.dispatch({
        type: "relabel",
        label: "b",
      }),
    );
    expect(renders).toBe(1);
    expect(result.current).toBe(0);

    act(() =>
      store.dispatch({
        type: "inc",
      }),
    );
    expect(renders).toBe(2);
    expect(result.current).toBe(1);
  });

  it("honors a custom equality: an equal object selection keeps its reference", () => {
    const store = make();
    let renders = 0;
    const { result } = renderHook(() => {
      renders += 1;
      return useStore(
        store,
        (s) => ({
          count: s.count,
        }),
        (a, b) => a.count === b.count,
      );
    });
    const first = result.current;
    expect(renders).toBe(1);

    act(() =>
      store.dispatch({
        type: "relabel",
        label: "c",
      }),
    );
    expect(renders).toBe(1);
    expect(result.current).toBe(first);

    act(() =>
      store.dispatch({
        type: "inc",
      }),
    );
    expect(renders).toBe(2);
    expect(result.current).not.toBe(first);
    expect(result.current).toEqual({
      count: 1,
    });
  });

  it("unmounting the hook deregisters its store subscription", () => {
    const inner = make();
    let active = 0;
    const store: Store<CounterState, CounterAction> = {
      snapshot: inner.snapshot,
      dispatch: inner.dispatch,
      subscribe: (listener) => {
        active += 1;
        const off = inner.subscribe(listener);
        return () => {
          active -= 1;
          off();
        };
      },
    };

    const { unmount } = renderHook(() => useStore(store, (s) => s.count));
    expect(active).toBe(1);

    unmount();
    expect(active).toBe(0);
  });
});
