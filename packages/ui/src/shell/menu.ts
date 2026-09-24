/**
 * L0 application-menu catalog — File / Edit / View / Help and their leaves.
 *
 * The leaf lists are fixed by the navigation model; each leaf names an action id
 * (resolved against the {@link ActionRegistry}) and, optionally, a display-only
 * keybinding. A `null` entry is a separator. A leaf whose action id is not bound
 * is dropped by {@link visibleMenu} — never rendered as a dead item.
 */

import type { MessageKey } from "../shared/i18n";
import type { ActionRegistry } from "./actions";

export type MenuGroupId = "file" | "edit" | "view" | "help";

export interface MenuLeaf {
  actionId: string;
  labelKey: MessageKey;
  binding?: string;
}

export interface MenuGroup {
  id: MenuGroupId;
  labelKey: MessageKey;
  /** Leaves in order; `null` is a separator. */
  leaves: readonly (MenuLeaf | null)[];
}

export const MENU: readonly MenuGroup[] = [
  {
    id: "file",
    labelKey: "menu.file",
    leaves: [
      {
        actionId: "file.open",
        labelKey: "menu.file.open",
        binding: "Ctrl O",
      },
      {
        actionId: "file.new-project",
        labelKey: "menu.file.new-project",
        binding: "Ctrl N",
      },
      {
        actionId: "file.settings",
        labelKey: "menu.file.settings",
        binding: "Ctrl ,",
      },
      null,
      {
        actionId: "file.close-window",
        labelKey: "menu.file.close-window",
        binding: "Ctrl W",
      },
      {
        actionId: "file.exit",
        labelKey: "menu.file.exit",
        binding: "Ctrl Q",
      },
    ],
  },
  {
    id: "edit",
    labelKey: "menu.edit",
    leaves: [
      {
        actionId: "edit.undo",
        labelKey: "menu.edit.undo",
        binding: "Ctrl Z",
      },
      {
        actionId: "edit.redo",
        labelKey: "menu.edit.redo",
        binding: "Ctrl Y",
      },
      null,
      {
        actionId: "edit.cut",
        labelKey: "menu.edit.cut",
        binding: "Ctrl X",
      },
      {
        actionId: "edit.copy",
        labelKey: "menu.edit.copy",
        binding: "Ctrl C",
      },
      {
        actionId: "edit.paste",
        labelKey: "menu.edit.paste",
        binding: "Ctrl V",
      },
      {
        actionId: "edit.select-all",
        labelKey: "menu.edit.select-all",
        binding: "Ctrl A",
      },
      null,
      {
        actionId: "edit.find",
        labelKey: "menu.edit.find",
        binding: "Ctrl F",
      },
      {
        actionId: "edit.find-next",
        labelKey: "menu.edit.find-next",
        binding: "F3",
      },
      {
        actionId: "edit.find-previous",
        labelKey: "menu.edit.find-previous",
        binding: "Shift F3",
      },
    ],
  },
  {
    id: "view",
    labelKey: "menu.view",
    leaves: [
      {
        actionId: "view.reload",
        labelKey: "menu.view.reload",
        binding: "Ctrl R",
      },
      null,
      {
        actionId: "view.actual-size",
        labelKey: "menu.view.actual-size",
        binding: "Ctrl 0",
      },
      {
        actionId: "view.zoom-in",
        labelKey: "menu.view.zoom-in",
        binding: "Ctrl +",
      },
      {
        actionId: "view.zoom-out",
        labelKey: "menu.view.zoom-out",
        binding: "Ctrl -",
      },
      null,
      {
        actionId: "view.copy-url",
        labelKey: "menu.view.copy-url",
      },
    ],
  },
  {
    id: "help",
    labelKey: "menu.help",
    leaves: [
      {
        actionId: "help.documentation",
        labelKey: "menu.help.documentation",
      },
      {
        actionId: "help.check-updates",
        labelKey: "menu.help.check-updates",
      },
      null,
      {
        actionId: "help.troubleshooting",
        labelKey: "menu.help.troubleshooting",
      },
      null,
      {
        actionId: "help.support",
        labelKey: "menu.help.support",
      },
      {
        actionId: "help.about",
        labelKey: "menu.help.about",
      },
    ],
  },
] as const;

/**
 * The menu as it should render for a given registry: leaves whose action is not
 * bound are removed, and separators that become leading, trailing, or
 * doubled after that removal are collapsed.
 */
export function visibleMenu(registry: ActionRegistry): MenuGroup[] {
  return MENU.map((group) => {
    const kept: (MenuLeaf | null)[] = [];
    for (const leaf of group.leaves) {
      if (leaf === null) {
        if (kept.length > 0 && kept[kept.length - 1] !== null) kept.push(null);
        continue;
      }
      if (registry.get(leaf.actionId)?.bound !== false && registry.has(leaf.actionId)) {
        kept.push(leaf);
      }
    }
    while (kept.length > 0 && kept[kept.length - 1] === null) kept.pop();
    return {
      ...group,
      leaves: kept,
    };
  });
}
