/**
 * Keymap dispatch — context stack, the pure resolver, and the three-layer merge.
 *
 * All pure and leaf-tier: no React, no DOM, no host. Key input resolves against
 * a **context stack** assembled from the focus path (outermost frame first), not
 * a single root handler. A binding fires only where its context predicate holds,
 * the most specific one wins, and an unresolved keystroke returns `unbound` so
 * the caller lets it fall through — a shell that swallows unhandled keys breaks
 * text input.
 */

/** One frame of the focus path and the context tags it contributes. */
export interface ContextFrame {
  /** A name for the frame, for the keymap surface and debugging. */
  readonly id: string;
  /** Context tags this frame adds, e.g. `["workspace"]`, `["dock", "sidebar"]`. */
  readonly contexts: readonly string[];
}

/** The focus path, outermost frame first. */
export type ContextStack = readonly ContextFrame[];

/**
 * Where a binding is live. Should be *monotonic* — once true for a stack prefix
 * it stays true as frames are added — so "satisfied deepest" is well defined.
 */
export type ContextPredicate = (stack: ContextStack) => boolean;

/** Live everywhere. */
export const always: ContextPredicate = () => true;

/** Live when some frame on the stack contributes `tag`. */
export function inContext(tag: string): ContextPredicate {
  return (stack) => stack.some((frame) => frame.contexts.includes(tag));
}

/** Live only when every listed tag is present. */
export function allContexts(...tags: readonly string[]): ContextPredicate {
  return (stack) => tags.every((tag) => inContext(tag)(stack));
}

/**
 * Specificity: the shortest stack prefix length (1..n) at which `predicate`
 * first holds, or -1 if it never holds — not even over the full stack. A more
 * specific predicate needs more context, so a higher number wins.
 */
export function satisfiedDepth(predicate: ContextPredicate, stack: ContextStack): number {
  for (let k = 1; k <= stack.length; k += 1) {
    if (predicate(stack.slice(0, k))) {
      return k;
    }
  }
  // an empty stack still lets an unconditional predicate through
  if (stack.length === 0 && predicate(stack)) {
    return 0;
  }
  return -1;
}

/** A chord sequence bound to an action. One entry per keystroke. */
export interface KeyBinding {
  /** The action this binding invokes. */
  readonly actionId: string;
  /** Normalized chords, e.g. `["Ctrl+Shift+J"]` or `["Ctrl+K", "Ctrl+S"]`. */
  readonly sequence: readonly string[];
  /** Where the binding is live. Defaults to `always`. */
  readonly when?: ContextPredicate;
}

/** Normalize a keyboard event into a chord string, e.g. `"Ctrl+Shift+J"`. */
export function eventToKeystroke(e: {
  key: string;
  ctrlKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
  metaKey?: boolean;
}): string {
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  if (e.metaKey) parts.push("Meta");
  parts.push(e.key.length === 1 ? e.key.toUpperCase() : e.key);
  return parts.join("+");
}

function isPrefix(prefix: readonly string[], sequence: readonly string[]): boolean {
  if (prefix.length > sequence.length) {
    return false;
  }
  return prefix.every((chord, i) => chord === sequence[i]);
}

/** The outcome of resolving a keystroke against the merged keymap in a context. */
export type Resolution =
  | {
      kind: "action";
      binding: KeyBinding;
    }
  | {
      kind: "pending";
      prefix: readonly string[];
    }
  | {
      kind: "unbound";
    };

/**
 * Resolve `keystroke` (appended to any `pending` prefix) against `keymap` over
 * `stack`. Pure. Steps mirror the model:
 *
 * 1. Candidates are bindings whose predicate holds over the stack and whose
 *    sequence has the typed sequence as a prefix.
 * 2. An exact match returns `action` — the winner is the most specific
 *    (predicate satisfied deepest), then the most recently layered.
 * 3. A strictly longer candidate returns `pending`; the caller holds the prefix
 *    and calls again on the next keystroke. A timeout or a cancel is the caller
 *    dropping the prefix and calling with `pending = []`.
 * 4. Nothing matches -> `unbound`; the caller lets the key fall through.
 */
export function resolve(
  keystroke: string,
  stack: ContextStack,
  keymap: readonly KeyBinding[],
  pending: readonly string[] = [],
): Resolution {
  const seq = [
    ...pending,
    keystroke,
  ];

  const candidates = keymap
    .map((binding) => ({
      binding,
      depth: satisfiedDepth(binding.when ?? always, stack),
    }))
    .filter(({ binding, depth }) => depth >= 0 && isPrefix(seq, binding.sequence));

  const exact = candidates.filter(({ binding }) => binding.sequence.length === seq.length);
  if (exact.length > 0) {
    let winner = exact[0];
    for (const candidate of exact) {
      const moreSpecific = candidate.depth > winner.depth;
      const laterOnTie =
        candidate.depth === winner.depth &&
        keymap.indexOf(candidate.binding) > keymap.indexOf(winner.binding);
      if (moreSpecific || laterOnTie) {
        winner = candidate;
      }
    }
    return {
      kind: "action",
      binding: winner.binding,
    };
  }

  if (candidates.some(({ binding }) => binding.sequence.length > seq.length)) {
    return {
      kind: "pending",
      prefix: seq,
    };
  }

  return {
    kind: "unbound",
  };
}

/** The three deterministic binding layers, merged in this fixed order. */
export type LayerName = "base" | "platform" | "user";

/** A layer entry: a binding to set, or an explicit disable of an action's binding. */
export type LayerEntry =
  | KeyBinding
  | {
      readonly actionId: string;
      readonly sequence: null;
    };

export interface BindingLayer {
  readonly name: LayerName;
  readonly bindings: readonly LayerEntry[];
}

export interface ResolvedBinding extends KeyBinding {
  /** Which layer supplied the effective binding, so an override reads as an override. */
  readonly layer: LayerName;
}

/**
 * Merge the layers into one binding table. Later layers replace a binding
 * of the same action id; an entry with `sequence: null` disables the action's
 * binding entirely. Order is always base -> platform -> user.
 */
export function mergeKeymap(layers: readonly BindingLayer[]): ResolvedBinding[] {
  const table = new Map<string, ResolvedBinding | null>();
  for (const layer of layers) {
    for (const entry of layer.bindings) {
      if (entry.sequence === null) {
        table.set(entry.actionId, null);
      } else {
        table.set(entry.actionId, {
          ...entry,
          layer: layer.name,
        });
      }
    }
  }
  return [
    ...table.values(),
  ].filter((binding): binding is ResolvedBinding => binding !== null);
}
