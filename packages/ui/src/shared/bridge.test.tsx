import { describe, expect, it, vi } from "vitest";
import {
  type ChannelEvent,
  createCoreClient,
  type Invocable,
  type InvokeFn,
  type ListenFn,
  type Outcome,
} from "./bridge";

describe("core bridge client", () => {
  it("marshals status to the capability_status IPC command", async () => {
    const invoke = vi.fn().mockResolvedValue("Cronus core 0.1.0 — ok");
    const client = createCoreClient(invoke as InvokeFn);

    await expect(client.status()).resolves.toBe("Cronus core 0.1.0 — ok");
    expect(invoke).toHaveBeenCalledWith("capability_status");
  });

  it("marshals version to the capability_version IPC command", async () => {
    const invoke = vi.fn().mockResolvedValue("0.1.0");
    const client = createCoreClient(invoke as InvokeFn);

    await expect(client.version()).resolves.toBe("0.1.0");
    expect(invoke).toHaveBeenCalledWith("capability_version");
  });

  it("marshals catalog to the capability_catalog IPC command", async () => {
    const descriptor: Invocable = {
      id: "core:board.list",
      name: "List cards",
      summary: "List every card on the board",
      group: "board",
      locus: "Semantic",
      binders: [],
      stability: "Shipped",
    };
    const invoke = vi.fn().mockResolvedValue([
      descriptor,
    ]);
    const client = createCoreClient(invoke as InvokeFn);

    await expect(client.catalog()).resolves.toEqual([
      descriptor,
    ]);
    expect(invoke).toHaveBeenCalledWith("capability_catalog");
  });

  it("marshals invoke to the capability_invoke IPC command with id and args", async () => {
    const outcome: Outcome = {
      Value: {
        Text: "card-1",
      },
    };
    const invoke = vi.fn().mockResolvedValue(outcome);
    const client = createCoreClient(invoke as InvokeFn);

    await expect(
      client.invoke({
        id: "core:board.add",
        args: {
          title: {
            Text: "Ship it",
          },
        },
      }),
    ).resolves.toEqual(outcome);
    expect(invoke).toHaveBeenCalledWith("capability_invoke", {
      id: "core:board.add",
      args: {
        title: {
          Text: "Ship it",
        },
      },
    });
  });

  it("marshals invoke with no args as an empty object, not undefined", async () => {
    const invoke = vi.fn().mockResolvedValue(null);
    const client = createCoreClient(invoke as InvokeFn);

    await client.invoke({
      id: "core:pane.focus-next",
    });
    expect(invoke).toHaveBeenCalledWith("capability_invoke", {
      id: "core:pane.focus-next",
      args: {},
    });
  });

  it("invoke resolves to null for the registry's real Unknown answer, never a fabricated Outcome", async () => {
    const invoke = vi.fn().mockResolvedValue(null);
    const client = createCoreClient(invoke as InvokeFn);

    await expect(
      client.invoke({
        id: "core:not-registered",
      }),
    ).resolves.toBeNull();
  });
});

describe("core bridge — wire types carry no field beyond JSON-serializable data", () => {
  it("an Invocable exercising every Locus/Stability shape round-trips through JSON unchanged", () => {
    const descriptors: Invocable[] = [
      {
        id: "core:board.list",
        name: "List cards",
        summary: "List every card on the board",
        group: "board",
        locus: "Semantic",
        binders: [
          {
            name: "id",
            kind: "Text",
            optional: false,
          },
        ],
        stability: "Shipped",
      },
      {
        id: "core:pane.focus-next",
        name: "Focus next",
        summary: "Move keyboard focus to the next panel",
        group: "pane",
        locus: "ClientLocal",
        binders: [],
        stability: "Shipped",
      },
      {
        id: "core:legacy.thing",
        name: "Legacy thing",
        summary: "Superseded by core:board.list",
        group: "legacy",
        locus: {
          HostOnly: {
            reason: "shell-owned marshalling, not core logic",
          },
        },
        binders: [],
        stability: {
          Retired: {
            superseded_by: "core:board.list",
          },
        },
      },
    ];

    for (const descriptor of descriptors) {
      const roundTripped = JSON.parse(JSON.stringify(descriptor)) as unknown;
      expect(roundTripped).toEqual(descriptor);
    }
  });

  it("every Outcome variant round-trips through JSON unchanged", () => {
    const outcomes: Outcome[] = [
      {
        Value: "Empty",
      },
      {
        Value: {
          Record: [
            [
              "id",
              {
                Text: "card-1",
              },
            ],
          ],
        },
      },
      {
        Stream: {
          channel: "board.events",
        },
      },
      {
        Rejected: {
          binder: "id",
          mode: "Absent",
          detail: "no id supplied",
        },
      },
      {
        Unavailable: {
          reason: "store connection failed",
        },
      },
    ];

    for (const outcome of outcomes) {
      const roundTripped = JSON.parse(JSON.stringify(outcome)) as unknown;
      expect(roundTripped).toEqual(outcome);
    }
  });
});

describe("core bridge client — settings", () => {
  it("marshals settings.get to capability_settings_get", async () => {
    const slice = {
      theme: "dark",
      colorScheme: "default",
      layout: null,
      keymapUser: {},
    };
    const invoke = vi.fn().mockResolvedValue(slice);
    const client = createCoreClient(invoke as InvokeFn);

    await expect(client.settings.get()).resolves.toEqual(slice);
    expect(invoke).toHaveBeenCalledWith("capability_settings_get");
  });

  it("marshals settings.set to capability_settings_set with a { patch } payload", async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    const client = createCoreClient(invoke as InvokeFn);

    await client.settings.set({
      theme: "light",
    });
    expect(invoke).toHaveBeenCalledWith("capability_settings_set", {
      patch: {
        theme: "light",
      },
    });
  });
});

const noInvoke = (() => Promise.reject(new Error("unused"))) as never;
const flush = async () => {
  await Promise.resolve();
  await Promise.resolve();
};

describe("core bridge — the push channel (subscribe)", () => {
  it("delivers messages and stops on the returned detach", async () => {
    let handler: ((e: { payload: ChannelEvent<number> }) => void) | undefined;
    const listen = ((_channel, h) => {
      handler = h as typeof handler;
      return Promise.resolve(() => {
        handler = undefined;
      });
    }) as ListenFn;
    const client = createCoreClient(noInvoke, listen);

    const seen: number[] = [];
    const detach = client.subscribe<number>("floors", (n) => seen.push(n));
    await flush();
    handler?.({
      payload: {
        type: "message",
        data: 1,
      },
    });
    handler?.({
      payload: {
        type: "message",
        data: 2,
      },
    });
    detach();
    handler?.({
      payload: {
        type: "message",
        data: 3,
      },
    });

    expect(seen).toEqual([
      1,
      2,
    ]);
  });

  it("fires onClose once when the host frames the channel closed", async () => {
    let handler: ((e: { payload: ChannelEvent<unknown> }) => void) | undefined;
    const listen = ((_channel, h) => {
      handler = h as typeof handler;
      return Promise.resolve(() => {});
    }) as ListenFn;
    const client = createCoreClient(noInvoke, listen);

    const onClose = vi.fn();
    client.subscribe("floors", () => {}, onClose);
    await flush();
    handler?.({
      payload: {
        type: "closed",
        reason: "core went away",
      },
    });
    handler?.({
      payload: {
        type: "closed",
        reason: "again",
      },
    });

    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledWith("core went away");
  });

  it("fires onClose when the channel fails to open", async () => {
    const listen = (() => Promise.reject(new Error("no such channel"))) as ListenFn;
    const client = createCoreClient(noInvoke, listen);

    const onClose = vi.fn();
    client.subscribe("floors", () => {}, onClose);
    await flush();

    expect(onClose).toHaveBeenCalledWith("no such channel");
  });

  it("with no listen injected, the channel cannot open", async () => {
    const client = createCoreClient(noInvoke);
    const onClose = vi.fn();
    client.subscribe("floors", () => {}, onClose);
    await flush();
    expect(onClose).toHaveBeenCalledWith("no event transport");
  });
});
