import { describe, expect, it } from "vitest";
import {
  allContexts,
  always,
  type BindingLayer,
  type ContextStack,
  eventToKeystroke,
  inContext,
  type KeyBinding,
  mergeKeymap,
  resolve,
  satisfiedDepth,
} from "./keymap";

const stack: ContextStack = [
  {
    id: "workspace",
    contexts: [
      "workspace",
    ],
  },
  {
    id: "sidebar",
    contexts: [
      "dock",
      "sidebar",
    ],
  },
  {
    id: "editor",
    contexts: [
      "panel",
      "editor",
    ],
  },
];

describe("context predicates and specificity", () => {
  it("inContext holds when some frame contributes the tag", () => {
    expect(inContext("workspace")(stack)).toBe(true);
    expect(inContext("editor")(stack)).toBe(true);
    expect(inContext("nope")(stack)).toBe(false);
  });

  it("allContexts holds only when every tag is present", () => {
    expect(allContexts("dock", "sidebar")(stack)).toBe(true);
    expect(allContexts("dock", "editor")(stack)).toBe(true);
    expect(allContexts("dock", "missing")(stack)).toBe(false);
  });

  it("satisfiedDepth is the shallowest prefix at which the predicate first holds", () => {
    expect(satisfiedDepth(inContext("workspace"), stack)).toBe(1);
    expect(satisfiedDepth(allContexts("dock", "sidebar"), stack)).toBe(2);
    expect(satisfiedDepth(inContext("editor"), stack)).toBe(3);
    expect(satisfiedDepth(inContext("never"), stack)).toBe(-1);
    expect(satisfiedDepth(always, stack)).toBe(1);
    expect(satisfiedDepth(always, [])).toBe(0);
  });
});

describe("resolve — the pure keymap resolver", () => {
  const open: KeyBinding = {
    actionId: "view.palette",
    sequence: [
      "Ctrl+Shift+J",
    ],
  };
  const save: KeyBinding = {
    actionId: "file.save",
    sequence: [
      "Ctrl+K",
      "Ctrl+S",
    ],
  };

  it("a full match returns the action", () => {
    expect(
      resolve("Ctrl+Shift+J", stack, [
        open,
      ]),
    ).toEqual({
      kind: "action",
      binding: open,
    });
  });

  it("a matched prefix returns pending, then completes on the next keystroke", () => {
    const first = resolve("Ctrl+K", stack, [
      save,
    ]);
    expect(first).toEqual({
      kind: "pending",
      prefix: [
        "Ctrl+K",
      ],
    });
    const second = resolve(
      "Ctrl+S",
      stack,
      [
        save,
      ],
      [
        "Ctrl+K",
      ],
    );
    expect(second).toEqual({
      kind: "action",
      binding: save,
    });
  });

  it("a cancel is the caller dropping the prefix — the next resolve starts fresh", () => {
    resolve("Ctrl+K", stack, [
      save,
    ]); // pending, then cancelled
    expect(
      resolve(
        "Ctrl+S",
        stack,
        [
          save,
        ],
        [],
      ),
    ).toEqual({
      kind: "unbound",
    });
  });

  it("an unbound keystroke falls through", () => {
    expect(
      resolve("Z", stack, [
        open,
        save,
      ]),
    ).toEqual({
      kind: "unbound",
    });
  });

  it("a binding whose predicate is false for the stack is not a candidate", () => {
    const scoped: KeyBinding = {
      actionId: "editor.format",
      sequence: [
        "Ctrl+E",
      ],
      when: inContext("terminal"),
    };
    expect(
      resolve("Ctrl+E", stack, [
        scoped,
      ]),
    ).toEqual({
      kind: "unbound",
    });
  });

  it("the most specific binding wins — predicate satisfied deepest in the stack", () => {
    const shallow: KeyBinding = {
      actionId: "a",
      sequence: [
        "Ctrl+P",
      ],
      when: inContext("workspace"),
    };
    const deep: KeyBinding = {
      actionId: "b",
      sequence: [
        "Ctrl+P",
      ],
      when: inContext("editor"),
    };
    const r = resolve("Ctrl+P", stack, [
      shallow,
      deep,
    ]);
    expect(r).toEqual({
      kind: "action",
      binding: deep,
    });
  });

  it("on a specificity tie, the most recently layered wins", () => {
    const earlier: KeyBinding = {
      actionId: "a",
      sequence: [
        "Ctrl+P",
      ],
      when: always,
    };
    const later: KeyBinding = {
      actionId: "b",
      sequence: [
        "Ctrl+P",
      ],
      when: always,
    };
    expect(
      resolve("Ctrl+P", stack, [
        earlier,
        later,
      ]),
    ).toEqual({
      kind: "action",
      binding: later,
    });
  });
});

describe("mergeKeymap — three deterministic layers", () => {
  const base: BindingLayer = {
    name: "base",
    bindings: [
      {
        actionId: "file.save",
        sequence: [
          "Ctrl+S",
        ],
      },
      {
        actionId: "view.palette",
        sequence: [
          "Ctrl+Shift+P",
        ],
      },
    ],
  };
  const platform: BindingLayer = {
    name: "platform",
    bindings: [
      {
        actionId: "view.palette",
        sequence: [
          "Meta+Shift+P",
        ],
      },
    ],
  };
  const user: BindingLayer = {
    name: "user",
    bindings: [
      {
        actionId: "file.save",
        sequence: [
          "Ctrl+Alt+S",
        ],
      },
      {
        actionId: "view.palette",
        sequence: null,
      },
    ],
  };

  it("a later layer replaces a binding of the same action id and records its origin", () => {
    const merged = mergeKeymap([
      base,
      platform,
    ]);
    const palette = merged.find((b) => b.actionId === "view.palette");
    expect(palette?.sequence).toEqual([
      "Meta+Shift+P",
    ]);
    expect(palette?.layer).toBe("platform");
  });

  it("an explicit null disables the action's binding entirely", () => {
    const merged = mergeKeymap([
      base,
      platform,
      user,
    ]);
    expect(merged.some((b) => b.actionId === "view.palette")).toBe(false);
    const save = merged.find((b) => b.actionId === "file.save");
    expect(save).toMatchObject({
      sequence: [
        "Ctrl+Alt+S",
      ],
      layer: "user",
    });
  });

  it("the merge order is fixed base -> platform -> user regardless of array order", () => {
    const a = mergeKeymap([
      base,
      platform,
    ]);
    const b = mergeKeymap([
      platform,
      base,
    ]);
    // reversing the input changes the winner — the caller must pass them in order
    expect(a.find((x) => x.actionId === "view.palette")?.layer).toBe("platform");
    expect(b.find((x) => x.actionId === "view.palette")?.layer).toBe("base");
  });
});

describe("eventToKeystroke", () => {
  it("normalizes modifiers in a fixed order and upper-cases a single key", () => {
    expect(
      eventToKeystroke({
        key: "j",
        ctrlKey: true,
        shiftKey: true,
      }),
    ).toBe("Ctrl+Shift+J");
    expect(
      eventToKeystroke({
        key: "J",
        ctrlKey: true,
      }),
    ).toBe("Ctrl+J");
    expect(
      eventToKeystroke({
        key: "k",
        ctrlKey: true,
        altKey: true,
        shiftKey: true,
        metaKey: true,
      }),
    ).toBe("Ctrl+Alt+Shift+Meta+K");
  });

  it("preserves a multi-character key name", () => {
    expect(
      eventToKeystroke({
        key: "Escape",
      }),
    ).toBe("Escape");
    expect(
      eventToKeystroke({
        key: "ArrowDown",
        altKey: true,
      }),
    ).toBe("Alt+ArrowDown");
  });
});
