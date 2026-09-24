import { describe, expect, it } from "vitest";
import {
  type CanvasNode,
  type Edge,
  type ObserverView,
  observerFor,
  projectionDrift,
  requestDevRun,
  validateConnections,
} from "./canvas";

describe("automation canvas projection", () => {
  it("detects projection drift between engine and canvas", () => {
    const drift = projectionDrift(
      [
        "a",
        "b",
        "c",
      ],
      [
        "a",
        "b",
      ],
    );
    expect(drift.missingFromCanvas).toEqual([
      "c",
    ]);
    expect(drift.missingFromEngine).toEqual([]);

    const clean = projectionDrift(
      [
        "a",
        "b",
      ],
      [
        "a",
        "b",
      ],
    );
    expect(clean.missingFromCanvas).toEqual([]);
    expect(clean.missingFromEngine).toEqual([]);
  });

  it("flags a trigger with an inbound edge (AC editing)", () => {
    const nodes: CanvasNode[] = [
      {
        id: "t",
        type: "trigger",
      },
      {
        id: "f",
        type: "filter",
      },
    ];
    const edges: Edge[] = [
      {
        from: "f",
        to: "t",
      },
    ];
    const issues = validateConnections(nodes, edges);
    expect(issues).toEqual([
      {
        kind: "trigger-has-inbound",
        node: "t",
      },
    ]);
  });

  it("flags an action that is not a leaf unless it has an error branch", () => {
    const nodes: CanvasNode[] = [
      {
        id: "a",
        type: "action",
      },
      {
        id: "next",
        type: "transform",
      },
      {
        id: "err",
        type: "action",
      },
    ];
    // action -> transform is a violation (not a leaf, not an error branch).
    expect(
      validateConnections(nodes, [
        {
          from: "a",
          to: "next",
        },
      ]),
    ).toEqual([
      {
        kind: "action-not-leaf",
        node: "a",
      },
    ]);
    // action -> err where err is an error branch is allowed.
    expect(
      validateConnections(
        nodes,
        [
          {
            from: "a",
            to: "err",
          },
        ],
        new Set([
          "err",
        ]),
      ),
    ).toEqual([]);
  });

  it("builds a dev-run request without executing", () => {
    const req = requestDevRun("node-3", "node-4");
    expect(req).toEqual({
      pinnedNode: "node-3",
      fromNode: "node-4",
      development: true,
    });
  });

  it("resolves the observer that catches a node, scoped over catch-all", () => {
    const observers: ObserverView[] = [
      {
        id: "scoped",
        kind: "error",
        scope: [
          "node-a",
        ],
      },
      {
        id: "catchall",
        kind: "error",
        scope: [],
      },
      {
        id: "status-obs",
        kind: "status",
        scope: [
          "node-a",
        ],
      },
    ];
    expect(observerFor(observers, "node-a", "error")).toBe("scoped");
    expect(observerFor(observers, "node-b", "error")).toBe("catchall");
    // No completion observer -> unhandled.
    expect(observerFor(observers, "node-a", "completion")).toBeNull();
  });
});
