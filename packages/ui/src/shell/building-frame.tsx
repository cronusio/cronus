/**
 * L0 · Building frame — the application-global chrome.
 *
 * A 34px title bar: app mark, the burger that opens the application menu,
 * sidebar toggle and history controls on the left; the file-tree toggle and
 * window controls on the right. The menu is a single dropdown whose four groups
 * fly their leaves out to the side — not a menu bar — so the bar stays narrow
 * enough to double as the window's drag region.
 *
 * Presentation only: menu-open state and every toggle are caller-owned; a leaf
 * click dispatches the bound action and asks the caller to close. Unbound leaves
 * are already removed by `visibleMenu`, so nothing here is a dead control.
 */

import { type Locale, translator } from "../shared/i18n";
import type { ActionRegistry } from "./actions";
import { Icon } from "./icons";
import { type MenuGroupId, visibleMenu } from "./menu";

export interface BuildingFrameProps {
  actions: ActionRegistry;
  /** Which menu group's flyout is open; `null` closes the whole dropdown. */
  openGroup?: MenuGroupId | null;
  /** Toggle the dropdown / switch the open group. */
  onOpenGroup?: (group: MenuGroupId | null) => void;
  onToggleSidebar?: () => void;
  onToggleRightDock?: () => void;
  /** Reflected on the toggles so an open region reads as pressed. */
  sidebarOpen?: boolean;
  rightDockOpen?: boolean;
  /** Whether the file-tree toggle is offered (a building-wide facility). */
  showRightDockToggle?: boolean;
  /** History affordances — inert until a navigation stack exists. */
  canGoBack?: boolean;
  canGoForward?: boolean;
  onBack?: () => void;
  onForward?: () => void;
  onMinimize?: () => void;
  onMaximize?: () => void;
  onClose?: () => void;
  locale?: Locale;
}

const TOOL =
  "flex h-6 w-6.5 flex-none items-center justify-center rounded-sm bg-transparent text-text-secondary transition-colors hover:bg-surface-hover hover:text-text-primary";

export function BuildingFrame({
  actions,
  openGroup = null,
  onOpenGroup,
  onToggleSidebar,
  onToggleRightDock,
  sidebarOpen = true,
  rightDockOpen = false,
  showRightDockToggle = true,
  canGoBack = false,
  canGoForward = false,
  onBack,
  onForward,
  onMinimize,
  onMaximize,
  onClose,
  locale = "en",
}: BuildingFrameProps) {
  const msg = translator(locale);
  const groups = visibleMenu(actions);
  const menuOpen = openGroup !== null;

  return (
    <>
      <div
        data-testid="building-frame"
        className="relative z-60 flex h-8.5 flex-none select-none items-center border-b border-border-subtle bg-surface-0"
      >
        <div className="box-border flex w-52.5 flex-none items-center pl-2.5">
          <img
            src="assets/cronus-icon.png"
            alt={msg("app.title")}
            className="mr-3 h-3.75 w-3.75 rounded-sm"
          />
          <button
            type="button"
            data-testid="burger"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            title={msg("frame.menu")}
            className={`${TOOL} ${menuOpen ? "bg-surface-hover text-text-primary" : ""}`}
            onClick={() => onOpenGroup?.(menuOpen ? null : "file")}
          >
            <Icon name="burger" />
          </button>

          <div className="ml-auto flex items-center gap-0.5">
            <button
              type="button"
              data-testid="toggle-sidebar"
              aria-pressed={sidebarOpen}
              title={msg("frame.toggle-sidebar")}
              className={`${TOOL} ${sidebarOpen ? "text-text-primary" : ""}`}
              onClick={onToggleSidebar}
            >
              <Icon name="panelLeft" />
            </button>
            <button
              type="button"
              data-testid="nav-back"
              title={msg("frame.back")}
              disabled={!canGoBack}
              className={`${TOOL} disabled:cursor-default disabled:text-text-disabled disabled:hover:bg-transparent disabled:hover:text-text-disabled`}
              onClick={onBack}
            >
              <Icon name="chevronLeft" size={14} />
            </button>
            <button
              type="button"
              data-testid="nav-forward"
              title={msg("frame.forward")}
              disabled={!canGoForward}
              className={`${TOOL} disabled:cursor-default disabled:text-text-disabled disabled:hover:bg-transparent disabled:hover:text-text-disabled`}
              onClick={onForward}
            >
              <Icon name="chevronRight" size={14} />
            </button>
          </div>
        </div>

        {/* drag region */}
        <div className="h-full flex-1" />

        {showRightDockToggle ? (
          <div className="mr-1.5 flex items-center gap-0.5">
            <button
              type="button"
              data-testid="toggle-files"
              aria-pressed={rightDockOpen}
              title={msg("frame.toggle-files")}
              className={`${TOOL} ${rightDockOpen ? "bg-surface-hover text-text-primary" : ""}`}
              onClick={onToggleRightDock}
            >
              <Icon name="panelRight" />
            </button>
          </div>
        ) : null}

        <div className="flex h-full items-stretch">
          <button
            type="button"
            data-testid="window-minimize"
            title={msg("frame.minimize")}
            className="w-11 border-none bg-transparent text-text-secondary transition-colors hover:bg-surface-hover"
            onClick={onMinimize}
          >
            <Icon name="minimize" size={12} className="mx-auto" />
          </button>
          <button
            type="button"
            data-testid="window-maximize"
            title={msg("frame.maximize")}
            className="w-11 border-none bg-transparent text-text-secondary transition-colors hover:bg-surface-hover"
            onClick={onMaximize}
          >
            <Icon name="maximize" size={11} className="mx-auto" />
          </button>
          <button
            type="button"
            data-testid="window-close"
            title={msg("frame.close")}
            className="w-11 border-none bg-transparent text-text-secondary transition-colors hover:bg-danger-strong hover:text-text-primary"
            onClick={onClose}
          >
            <Icon name="close" size={12} className="mx-auto" />
          </button>
        </div>
      </div>

      {menuOpen ? (
        <ApplicationMenu
          groups={groups}
          openGroup={openGroup}
          onOpenGroup={onOpenGroup}
          actions={actions}
          locale={locale}
        />
      ) : null}
    </>
  );
}

interface ApplicationMenuProps {
  groups: ReturnType<typeof visibleMenu>;
  openGroup: MenuGroupId | null;
  onOpenGroup?: (group: MenuGroupId | null) => void;
  actions: ActionRegistry;
  locale: Locale;
}

/**
 * The burger dropdown: four group rows, each flying its leaves out to the right.
 *
 * Fully controlled — which flyout is showing *is* `openGroup`, so hovering a row
 * is the same intent as the caller switching groups. Holding it in local state
 * instead would let the two disagree after a re-render.
 */
function ApplicationMenu({
  groups,
  openGroup,
  onOpenGroup,
  actions,
  locale,
}: ApplicationMenuProps) {
  const msg = translator(locale);

  return (
    <>
      {/* click-away scrim */}
      <button
        type="button"
        aria-label={msg("frame.close-menu")}
        className="fixed inset-0 z-55 cursor-default bg-transparent"
        onClick={() => onOpenGroup?.(null)}
      />
      <div
        data-testid="menu-bar"
        className="absolute top-9.5 left-2.5 z-58 min-w-47 rounded-md border border-border-strong bg-surface-5 p-1.25 shadow-menu"
      >
        {groups.map((group) => {
          const open = openGroup === group.id;
          return (
            <div key={group.id} data-testid={`menu-group-${group.id}`} className="relative">
              <button
                type="button"
                className={`flex w-full cursor-default items-center rounded-sm px-2.5 py-1.5 text-left text-base ${
                  open ? "bg-surface-active" : "bg-transparent"
                }`}
                onMouseEnter={() => onOpenGroup?.(group.id)}
                onFocus={() => onOpenGroup?.(group.id)}
                onClick={() => onOpenGroup?.(group.id)}
              >
                {msg(group.labelKey)}
                <span className="ml-auto flex pl-6.5 text-text-muted">
                  <Icon name="caret" size={13} />
                </span>
              </button>

              {open ? (
                <div className="-top-1.5 absolute left-full z-59 ml-1.25 min-w-57.5 rounded-md border border-border-strong bg-surface-5 p-1.25 shadow-menu">
                  {group.leaves.map((leaf, i) =>
                    leaf === null ? (
                      <div
                        key={`${group.id}-sep-${i}`}
                        className="mx-1.5 my-1.25 h-px bg-border-subtle"
                      />
                    ) : (
                      <button
                        key={leaf.actionId}
                        type="button"
                        data-testid={`menu-leaf-${leaf.actionId}`}
                        className="flex w-full items-center rounded-sm bg-transparent px-2.5 py-1.5 text-left text-base transition-colors hover:bg-surface-active"
                        onClick={() => {
                          actions.get(leaf.actionId)?.run();
                          onOpenGroup?.(null);
                        }}
                      >
                        {msg(leaf.labelKey)}
                        {leaf.binding ? (
                          <span className="ml-auto pl-6 text-text-muted text-xs">
                            {leaf.binding}
                          </span>
                        ) : null}
                      </button>
                    ),
                  )}
                </div>
              ) : null}
            </div>
          );
        })}
      </div>
    </>
  );
}
