/**
 * L0 · Right-edge project file-tree dock (toggleable).
 *
 * Presentation only: a read-only projection of the active floor's workspace
 * tree. Git-ignored entries render dimmed. Open/reveal actions bind to
 * shell/core capabilities as they ship — in this slice the rows are
 * inert. Dock visibility is caller-owned view state.
 */

import { useState } from "react";
import { type Locale, translator } from "../shared/i18n";

export interface FileNode {
  name: string;
  kind: "dir" | "file";
  /** Dimmed in the tree when true (matches a `.gitignore` rule). */
  ignored?: boolean;
  children?: readonly FileNode[];
}

export interface RightDockProps {
  open: boolean;
  floorName?: string;
  tree: readonly FileNode[];
  locale?: Locale;
}

type FilterMode = "names" | "contents";

function Row({ node, depth }: { node: FileNode; depth: number }) {
  return (
    <>
      <div
        data-testid={`file-${node.name}`}
        data-ignored={node.ignored || undefined}
        className={`flex items-center gap-1.5 rounded-sm px-2 py-1 text-sm hover:bg-surface-2 ${
          node.ignored ? "text-text-muted italic" : "text-text-secondary"
        }`}
        style={{
          paddingLeft: `calc(var(--space-2) + ${depth} * var(--space-3))`,
        }}
      >
        <span aria-hidden="true">{node.kind === "dir" ? "▸" : "·"}</span>
        <span className="truncate">{node.name}</span>
      </div>
      {node.children?.map((child) => (
        <Row key={child.name} node={child} depth={depth + 1} />
      ))}
    </>
  );
}

export function RightDock({ open, floorName, tree, locale = "en" }: RightDockProps) {
  const msg = translator(locale);
  const [filter, setFilter] = useState<FilterMode>("names");
  if (!open) return null;

  return (
    <div
      data-testid="right-dock"
      className="flex w-85 flex-none flex-col border-l border-border-subtle bg-surface-0"
    >
      <div className="flex items-center gap-2 border-b border-border-subtle p-3">
        <span className="text-sm font-semibold text-text-primary">
          {floorName ?? msg("dock.title")}
        </span>
      </div>
      <div className="p-3">
        <input
          data-testid="dock-find"
          placeholder={msg("dock.find-files")}
          className="w-full rounded-md border border-border-subtle bg-surface-1 px-2.5 py-1.5 text-sm text-text-secondary outline-none"
        />
      </div>
      <div className="mx-3 mb-2 flex gap-1 rounded-md border border-border-subtle bg-surface-1 p-0.5">
        {(
          [
            "names",
            "contents",
          ] as const
        ).map((mode) => (
          <button
            key={mode}
            type="button"
            data-testid={`dock-filter-${mode}`}
            aria-pressed={filter === mode}
            className="flex-1 rounded-sm py-1 text-xs text-text-muted aria-pressed:bg-surface-3 aria-pressed:text-text-primary"
            onClick={() => setFilter(mode)}
          >
            {msg(mode === "names" ? "dock.filter.names" : "dock.filter.contents")}
          </button>
        ))}
      </div>
      <div className="flex-1 overflow-y-auto px-1.5 pb-3">
        {tree.map((node) => (
          <Row key={node.name} node={node} depth={0} />
        ))}
      </div>
    </div>
  );
}
