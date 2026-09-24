/**
 * The action registry — the command vocabulary.
 *
 * A `ShellAction` is a named command independent of how it is triggered (a menu
 * leaf, a keybinding, a palette row, or code). A control invokes an action by id
 * and never holds the behaviour, so the same action is renderable in a menu, the
 * palette, and the keymap surface without bespoke wiring. Two gates decide
 * whether a surface may show an action: `bound` (is the capability shipped)
 * and `when` (is it live in the current context).
 *
 * This module is still narrow: it carries the vocabulary and the two gates. The
 * keystroke resolution that consumes `when` lives in `../shared/keymap`. The one
 * IPC-adjacent piece it does carry is {@link actionsFromCatalog} — deriving
 * `Semantic` actions from the core's own catalog is what keeps this registry
 * from becoming a second, hand-maintained copy of it.
 */

import type { Invocable } from "../shared/bridge";
import type { MessageKey } from "../shared/i18n";
import { always, type ContextPredicate, type ContextStack } from "../shared/keymap";

/**
 * An action's user-visible label — one of two channels, never both. `key`
 * resolves through this frontend's own closed i18n catalog (a hand-declared
 * action, whose label this frontend authored). `text` is already resolved —
 * for a `Semantic` action sourced from the core's own catalog, whose `name`
 * is a fixed string the core registry owns; there is no `MessageKey` this
 * frontend's catalog could look it up by, and inventing synthetic keys for a
 * runtime-discovered id would defeat the closed-catalog point of `MessageKey`
 * entirely.
 */
export type ActionLabel =
  | {
      key: MessageKey;
    }
  | {
      text: string;
    };

/** Resolve an action's label to a plain string, whichever channel it uses. */
export function resolveLabel(msg: (key: MessageKey) => string, label: ActionLabel): string {
  return "key" in label ? msg(label.key) : label.text;
}

/** One registered command. */
export interface ShellAction {
  /** Stable namespaced id, e.g. `"file.settings"`. */
  id: string;
  /** The user-visible label. Mandatory — an action is always describable. */
  label: ActionLabel;
  /** What the action does. Presentation-only callers pass a no-op or an intent. */
  run: () => void;
  /** Current keybinding, display-only (e.g. `"Ctrl ,"`). */
  binding?: string;
  /** Whether the action is bound to a shipped capability. An unbound action is
   *  hidden from every surface — never rendered as a dead control. */
  bound?: boolean;
  /** Where the action is live. Absent means everywhere. A predicate that
   *  is false for the current context hides the action and drops its binding. */
  when?: ContextPredicate;
}

/** An immutable lookup over registered actions. */
export interface ActionRegistry {
  get(id: string): ShellAction | undefined;
  /** All actions that are bound — the only ones any surface may render. */
  bound(): ShellAction[];
  /** The bound actions whose `when` predicate holds over `stack`. */
  live(stack: ContextStack): ShellAction[];
  has(id: string): boolean;
}

/** Build a registry from a list. A later entry with the same id overrides. */
export function createActionRegistry(actions: readonly ShellAction[]): ActionRegistry {
  const byId = new Map<string, ShellAction>();
  for (const a of actions) {
    byId.set(a.id, {
      bound: true,
      ...a,
    });
  }
  const boundActions = () =>
    [
      ...byId.values(),
    ].filter((a) => a.bound !== false);
  return {
    get: (id) => byId.get(id),
    has: (id) => byId.has(id),
    bound: boundActions,
    live: (stack) => boundActions().filter((a) => (a.when ?? always)(stack)),
  };
}

/** Whether an action id resolves to a bound command (render gate). */
export function isBound(registry: ActionRegistry, id: string): boolean {
  return registry.get(id)?.bound !== false && registry.has(id);
}

/**
 * Build one `ShellAction` per `Semantic` descriptor in `catalog` — the
 * registry is not a second catalog: an id is visible here
 * because, and only because, the fed-in catalog snapshot currently reports
 * it. There is no separate `bound: false` entry for an id the catalog
 * dropped — it simply produces no `ShellAction` at all, so a stale caller
 * holding an old id sees it vanish from `bound()`/`live()` the moment the
 * catalog snapshot that built this list moves on (the catalog cache's own
 * refresh is what keeps that snapshot current).
 *
 * `ClientLocal` (and every other locus) stays out: those actions have no
 * generic dispatch path this function could wire, and a surface declares
 * them locally instead — the terminal UI's own precedent for the same
 * locus split, applied here to the action registry rather than a slash
 * catalog.
 *
 * `dispatch` is the one seam this function needs — a plain `(id) => void`,
 * not a `CoreClient` — so this module stays free of any IPC dependency
 * (its own module doc: "carries the vocabulary and the two gates," nothing
 * else). A descriptor declaring required arguments (`binders`) still
 * becomes an action that dispatches with none: no surface in this codebase
 * collects arguments for a menu/palette click yet, a disclosed limitation
 * rather than a reason to leave such descriptors out of the catalog-derived
 * set entirely.
 */
export function actionsFromCatalog(
  catalog: readonly Invocable[],
  dispatch: (id: string) => void,
): ShellAction[] {
  return catalog
    .filter((invocable) => invocable.locus === "Semantic")
    .map((invocable) => ({
      id: invocable.id,
      label: {
        text: invocable.name,
      },
      run: () => dispatch(invocable.id),
    }));
}
