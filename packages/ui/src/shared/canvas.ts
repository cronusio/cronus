/**
 * Automation canvas — pure projection logic for the visual pipeline editor.
 *
 * Presentation only: the canvas is a projection of the automation engine and adds
 * no execution semantics. Live node state and traces arrive from the engine
 * over the bridge; this module holds the render/validation rules — projection
 * fidelity, connection-rule validation for editing, dev-run
 * request construction that never executes, and observer scope
 * resolution.
 */

/** A node type in the pipeline (mirrors the engine taxonomy). */
export type NodeType =
  | "trigger"
  | "filter"
  | "transform"
  | "branch"
  | "action"
  | "delay"
  | "aggregate"
  | "loop"
  | "subpipeline"
  | "observer";

export interface CanvasNode {
  id: string;
  type: NodeType;
}

/**
 * Projection fidelity check: the canvas graph must faithfully represent the
 * engine's node set. Returns the ids present in one side but not the other; a
 * non-empty result is a consistency violation the canvas must refresh away.
 */
export function projectionDrift(
  engineNodeIds: readonly string[],
  canvasNodeIds: readonly string[],
): {
  missingFromCanvas: string[];
  missingFromEngine: string[];
} {
  const engine = new Set(engineNodeIds);
  const canvas = new Set(canvasNodeIds);
  return {
    missingFromCanvas: engineNodeIds.filter((id) => !canvas.has(id)),
    missingFromEngine: canvasNodeIds.filter((id) => !engine.has(id)),
  };
}

export interface Edge {
  from: string;
  to: string;
}

/** A connection-rule violation surfaced during editing. */
export type ConnectionIssue =
  | {
      kind: "trigger-has-inbound";
      node: string;
    }
  | {
      kind: "action-not-leaf";
      node: string;
    };

/**
 * Validate the connection rules for an explicit pipeline: a trigger node
 * may have only outbound edges; an action node must be a leaf unless it is followed
 * by an error branch. Returns all violations (empty = valid).
 */
export function validateConnections(
  nodes: readonly CanvasNode[],
  edges: readonly Edge[],
  errorBranchNodes: ReadonlySet<string> = new Set(),
): ConnectionIssue[] {
  const issues: ConnectionIssue[] = [];
  const byId = new Map(
    nodes.map((n) => [
      n.id,
      n,
    ]),
  );
  for (const node of nodes) {
    if (node.type === "trigger") {
      if (edges.some((e) => e.to === node.id)) {
        issues.push({
          kind: "trigger-has-inbound",
          node: node.id,
        });
      }
    }
    if (node.type === "action") {
      const outbound = edges.filter((e) => e.from === node.id);
      const onlyErrorBranch = outbound.every((e) => {
        const target = byId.get(e.to);
        return target !== undefined && errorBranchNodes.has(target.id);
      });
      if (outbound.length > 0 && !onlyErrorBranch) {
        issues.push({
          kind: "action-not-leaf",
          node: node.id,
        });
      }
    }
  }
  return issues;
}

/** A development partial-run request the canvas hands to the engine. */
export interface DevRunRequest {
  pinnedNode: string;
  fromNode: string;
  development: true;
}

/**
 * Construct a dev-run request. The canvas only *requests* a partial run from
 * a pinned node; it never executes. Actions run dry unless explicitly opted
 * live downstream in the engine — that is not the canvas's concern.
 */
export function requestDevRun(pinnedNode: string, fromNode: string): DevRunRequest {
  return {
    pinnedNode,
    fromNode,
    development: true,
  };
}

export interface ObserverView {
  id: string;
  kind: "error" | "status" | "completion";
  /** Covered node ids; empty = catch-all (*unhandled*). */
  scope: readonly string[];
}

/**
 * Resolve which observer catches a node's lifecycle event of a given kind:
 * a scoped observer covering the node wins over a catch-all one; `null` if none
 * handles it (stop-on-failure then applies).
 */
export function observerFor(
  observers: readonly ObserverView[],
  node: string,
  kind: ObserverView["kind"],
): string | null {
  const ofKind = observers.filter((o) => o.kind === kind);
  const scoped = ofKind.find((o) => o.scope.includes(node));
  if (scoped) return scoped.id;
  const catchAll = ofKind.find((o) => o.scope.length === 0);
  return catchAll ? catchAll.id : null;
}
