/**
 * Delegated selection surface + the command palette (its first delegate).
 *
 * `SelectionSurface` owns the generic concerns — query input, grouped result
 * list, keyboard open/close, confirm/cancel. A `SelectionDelegate` supplies the
 * specifics: how items are sourced from the query, how a row renders, and what
 * confirming an item does. A new chooser is a new delegate, not a new widget.
 *
 * Presentation only: opening/closing is caller-owned view state; nothing here
 * round-trips to the core. Results that reference live data read the caller's
 * injected projections.
 */

import { useState } from "react";
import { type Locale, type MessageKey, translator } from "../shared/i18n";
import { SIDEBAR_TABS, type SidebarTab } from "../shared/navigation";

/** One selectable row. */
export interface SelectionItem {
  id: string;
  /** Group heading key this row sits under. */
  groupKey: MessageKey;
  label: string;
  secondary?: string;
  binding?: string;
  /** Invoked on confirm. */
  confirm: () => void;
}

/** Supplies the behaviour of a `SelectionSurface`. */
export interface SelectionDelegate {
  placeholderKey: MessageKey;
  /** Source + rank items for the current query (empty query → the default set). */
  items(query: string): SelectionItem[];
}

export interface SelectionSurfaceProps {
  open: boolean;
  delegate: SelectionDelegate;
  onClose?: () => void;
  locale?: Locale;
}

export function SelectionSurface({
  open,
  delegate,
  onClose,
  locale = "en",
}: SelectionSurfaceProps) {
  const msg = translator(locale);
  const [query, setQuery] = useState("");
  if (!open) return null;

  const items = delegate.items(query);
  const groups = new Map<MessageKey, SelectionItem[]>();
  for (const item of items) {
    const bucket = groups.get(item.groupKey) ?? [];
    bucket.push(item);
    groups.set(item.groupKey, bucket);
  }

  const close = () => {
    setQuery("");
    onClose?.();
  };

  return (
    <div
      data-testid="selection-surface"
      className="absolute inset-0 z-130 flex justify-center bg-surface-0/60 pt-27.5"
    >
      <button
        type="button"
        aria-label="close"
        data-testid="selection-scrim"
        className="absolute inset-0 cursor-default border-none bg-transparent"
        onClick={close}
      />
      <div className="relative flex w-[min(720px,88%)] flex-col overflow-hidden rounded-lg border border-border-strong bg-surface-2 shadow-overlay">
        <input
          data-testid="selection-input"
          value={query}
          placeholder={msg(delegate.placeholderKey)}
          className="border-b border-border-subtle bg-transparent px-5 py-4 text-lg text-text-primary outline-none"
          // biome-ignore lint/a11y/noAutofocus: a command palette is expected to take focus on open
          autoFocus
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") close();
          }}
        />
        <div className="max-h-95 overflow-y-auto p-2.5">
          {items.length === 0 ? (
            <p data-testid="selection-empty" className="px-3 py-2.5 text-sm text-text-muted">
              {msg("palette.empty")}
            </p>
          ) : (
            [
              ...groups.entries(),
            ].map(([groupKey, rows]) => (
              <div key={groupKey}>
                <div className="px-3 pt-2.5 pb-1.5 text-xs font-semibold tracking-wide text-text-muted">
                  {msg(groupKey)}
                </div>
                {rows.map((row) => (
                  <button
                    key={row.id}
                    type="button"
                    data-testid={`selection-item-${row.id}`}
                    className="flex w-full items-center gap-3 rounded-md px-3 py-2.5 text-left hover:bg-surface-3"
                    onClick={() => {
                      row.confirm();
                      close();
                    }}
                  >
                    <span className="text-sm text-text-primary">{row.label}</span>
                    {row.secondary ? (
                      <span className="text-xs text-text-muted">{row.secondary}</span>
                    ) : null}
                    {row.binding ? (
                      <span className="ml-auto text-xs text-text-muted">{row.binding}</span>
                    ) : null}
                  </button>
                ))}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}

// ── Command palette · the built-in delegate ─────────────────────────────────

export interface CommandPaletteProps {
  open: boolean;
  onClose?: () => void;
  /** Recent offices, most-recent first. */
  recentOffices?: readonly {
    id: string;
    name: string;
    hint?: string;
  }[];
  onGoToOffice?: (id: string) => void;
  onGoToSubsystem?: (tab: SidebarTab) => void;
  onOpenSettings?: () => void;
  /** Bound actions from the shell registry, already filtered to shipped capabilities. */
  actions?: readonly {
    id: string;
    label: string;
    binding?: string;
    run: () => void;
  }[];
  locale?: Locale;
}

const fuzzy = (haystack: string, needle: string) =>
  needle === "" || haystack.toLowerCase().includes(needle.toLowerCase());

/** The command-palette delegate: offices + go-to-subsystem + settings + actions. */
export function commandPaletteDelegate(
  props: Omit<CommandPaletteProps, "open" | "onClose">,
  msg: (k: MessageKey) => string,
): SelectionDelegate {
  const { recentOffices = [], onGoToOffice, onGoToSubsystem, onOpenSettings, actions = [] } = props;
  return {
    placeholderKey: "palette.placeholder",
    items(query) {
      const out: SelectionItem[] = [];
      for (const o of recentOffices) {
        if (fuzzy(o.name, query)) {
          out.push({
            id: `office:${o.id}`,
            groupKey: "palette.group.offices",
            label: o.name,
            secondary: o.hint,
            confirm: () => onGoToOffice?.(o.id),
          });
        }
      }
      for (const tab of SIDEBAR_TABS) {
        const label = msg(`nav.${tab}` as MessageKey);
        if (fuzzy(label, query)) {
          out.push({
            id: `subsystem:${tab}`,
            groupKey: "palette.group.subsystems",
            label,
            confirm: () => onGoToSubsystem?.(tab),
          });
        }
      }
      if (fuzzy(msg("menu.file.settings"), query)) {
        out.push({
          id: "settings:open",
          groupKey: "palette.group.settings",
          label: msg("menu.file.settings"),
          binding: "Ctrl ,",
          confirm: () => onOpenSettings?.(),
        });
      }
      for (const a of actions) {
        if (fuzzy(a.label, query)) {
          out.push({
            id: `action:${a.id}`,
            groupKey: "palette.group.actions",
            label: a.label,
            binding: a.binding,
            confirm: a.run,
          });
        }
      }
      return out;
    },
  };
}

export function CommandPalette({ open, onClose, locale = "en", ...rest }: CommandPaletteProps) {
  const msg = translator(locale);
  return (
    <SelectionSurface
      open={open}
      onClose={onClose}
      locale={locale}
      delegate={commandPaletteDelegate(rest, msg)}
    />
  );
}
