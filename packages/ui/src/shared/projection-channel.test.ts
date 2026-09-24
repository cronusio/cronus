import { describe, expect, it, vi } from "vitest";
import { type ChannelEvent, createCoreClient, type ListenFn } from "./bridge";
import { createProjectionStore } from "./projection";
import { bindProjectionChannel } from "./projection-channel";

const noInvoke = (() => Promise.reject(new Error("no invoke in this test"))) as never;

const flush = async () => {
  await Promise.resolve();
  await Promise.resolve();
};

function fakeListen() {
  const handlers = new Map<string, (event: { payload: ChannelEvent<unknown> }) => void>();
  let mode: "open" | "reject" = "open";
  const listen: ListenFn = ((channel, handler) => {
    if (mode === "reject") {
      return Promise.reject(new Error("host refused the channel"));
    }
    handlers.set(channel, handler as (event: { payload: ChannelEvent<unknown> }) => void);
    return Promise.resolve(() => {
      handlers.delete(channel);
    });
  }) as ListenFn;
  return {
    listen,
    emit<T>(channel: string, event: ChannelEvent<T>) {
      handlers.get(channel)?.({
        payload: event as ChannelEvent<unknown>,
      });
    },
    isOpen: (channel: string) => handlers.has(channel),
    reject() {
      mode = "reject";
    },
  };
}

describe("bindProjectionChannel — the seam's event direction and liveness", () => {
  it("opening puts the store in pending; a message fulfils it and notifies", async () => {
    const fake = fakeListen();
    const client = createCoreClient(noInvoke, fake.listen);
    const store = createProjectionStore<number>();
    let notifications = 0;
    store.subscribe(() => {
      notifications += 1;
    });

    bindProjectionChannel(client, "floors", store);
    expect(store.snapshot().kind).toBe("pending");

    await flush();
    fake.emit<number>("floors", {
      type: "message",
      data: 42,
    });
    expect(store.snapshot()).toEqual({
      kind: "loaded",
      data: 42,
    });
    expect(notifications).toBe(2); // request + fulfill
  });

  it("a subscription that fails to open moves the projection to unavailable", async () => {
    const fake = fakeListen();
    fake.reject();
    const client = createCoreClient(noInvoke, fake.listen);
    const store = createProjectionStore<number>();

    bindProjectionChannel(client, "badges", store);
    await flush();

    expect(store.snapshot()).toEqual({
      kind: "unavailable",
      reason: "host refused the channel",
    });
  });

  it("a host-reported close moves it to unavailable and detaches the listener", async () => {
    const fake = fakeListen();
    const client = createCoreClient(noInvoke, fake.listen);
    const store = createProjectionStore<number[]>();

    bindProjectionChannel(client, "tree", store);
    await flush();
    fake.emit<number[]>("tree", {
      type: "message",
      data: [
        1,
      ],
    });
    expect(store.snapshot()).toEqual({
      kind: "loaded",
      data: [
        1,
      ],
    });

    fake.emit<number[]>("tree", {
      type: "closed",
      reason: "core channel dropped",
    });
    expect(store.snapshot()).toEqual({
      kind: "unavailable",
      reason: "core channel dropped",
    });
    expect(fake.isOpen("tree")).toBe(false);
  });

  it("re-establishing re-requests — a fresh pending, not a mid-stream resume", async () => {
    const fake = fakeListen();
    const client = createCoreClient(noInvoke, fake.listen);
    const store = createProjectionStore<number[]>();

    bindProjectionChannel(client, "tree", store);
    await flush();
    fake.emit<number[]>("tree", {
      type: "message",
      data: [
        1,
      ],
    });
    fake.emit<number[]>("tree", {
      type: "closed",
      reason: "dropped",
    });
    expect(store.snapshot().kind).toBe("unavailable");

    bindProjectionChannel(client, "tree", store);
    expect(store.snapshot().kind).toBe("pending"); // not the stale loaded value

    await flush();
    fake.emit<number[]>("tree", {
      type: "message",
      data: [
        1,
        2,
      ],
    });
    expect(store.snapshot()).toEqual({
      kind: "loaded",
      data: [
        1,
        2,
      ],
    });
  });

  it("the detach function stops delivery", async () => {
    const fake = fakeListen();
    const client = createCoreClient(noInvoke, fake.listen);
    const store = createProjectionStore<number>();

    const detach = bindProjectionChannel(client, "recent", store);
    await flush();
    detach();
    expect(fake.isOpen("recent")).toBe(false);

    fake.emit<number>("recent", {
      type: "message",
      data: 9,
    });
    expect(store.snapshot().kind).toBe("pending"); // no fulfill after detach
  });

  it("no timer drives a re-request after a close", async () => {
    vi.useFakeTimers();
    try {
      const fake = fakeListen();
      const client = createCoreClient(noInvoke, fake.listen);
      const store = createProjectionStore<number>();

      bindProjectionChannel(client, "t", store);
      await flush();
      fake.emit<number>("t", {
        type: "closed",
        reason: "gone",
      });
      expect(store.snapshot().kind).toBe("unavailable");

      vi.advanceTimersByTime(120_000);
      expect(store.snapshot()).toEqual({
        kind: "unavailable",
        reason: "gone",
      });
    } finally {
      vi.useRealTimers();
    }
  });
});
