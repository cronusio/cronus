/**
 * The keymap surface — every action with its effective binding and the layer it
 * came from, so a user can see *why* a key does what it does.
 *
 * Presentation only: it renders a merged binding table; it neither merges nor
 * persists one. Merging is the runtime's; persisting the user layer is the
 * host's.
 */

import { type Locale, type MessageKey, translator } from "../shared/i18n";
import type { ResolvedBinding } from "../shared/keymap";

export interface KeymapSurfaceProps {
  /** The merged binding table (base -> platform -> user). */
  bindings: readonly ResolvedBinding[];
  /** Resolve an action id to its label key; an unknown id renders as the raw id. */
  labelFor: (actionId: string) => MessageKey | undefined;
  locale?: Locale;
}

export function KeymapSurface({ bindings, labelFor, locale = "en" }: KeymapSurfaceProps) {
  const msg = translator(locale);
  return (
    <ul data-testid="keymap-surface" className="flex flex-col gap-1">
      {bindings.map((binding) => {
        const key = labelFor(binding.actionId);
        return (
          <li
            key={binding.actionId}
            data-testid={`keymap-row-${binding.actionId}`}
            data-layer={binding.layer}
            className="flex items-center justify-between gap-4 px-2 py-1 text-sm"
          >
            <span className="text-text-primary">{key ? msg(key) : binding.actionId}</span>
            <span className="flex items-center gap-2">
              <span className="rounded-sm bg-surface-2 px-1.5 py-0.5 font-mono text-xs text-text-secondary">
                {binding.sequence.join(" ")}
              </span>
              <span
                data-testid={`keymap-origin-${binding.actionId}`}
                className="text-xs text-text-muted"
              >
                {binding.layer}
              </span>
            </span>
          </li>
        );
      })}
    </ul>
  );
}
