/**
 * Localization: externalized UI strings with English fallback.
 *
 * Every user-facing string resolves through a message catalog — components
 * never hardcode visible text. Catalogs ship per locale; a key missing from
 * the active locale falls back to English so the UI never renders a blank.
 */

/** Supported locales; the catalog is extensible. */
export type Locale = "en" | "ru";

export const DEFAULT_LOCALE: Locale = "en";

/** Message keys — typed so a missing key is a compile error, not a blank. */
export type MessageKey =
  | "app.title"
  | "status.connecting"
  // Workbench surfaces carried over from the first desktop slice
  | "surface.office"
  | "surface.board"
  | "surface.chat"
  | "surface.editor"
  | "surface.dashboard"
  | "surface.empty"
  | "office.empty"
  | "dashboard.statistics"
  | "dashboard.building"
  | "dashboard.offices"
  | "dashboard.active-agents"
  | "dashboard.cards"
  // ── Shell frame ──
  // L0 frame chrome
  | "frame.menu"
  | "frame.back"
  | "frame.forward"
  | "frame.toggle-sidebar"
  | "frame.toggle-files"
  | "frame.search"
  | "frame.minimize"
  | "frame.maximize"
  | "frame.close"
  // L0 application menu
  | "menu.file"
  | "menu.file.open"
  | "menu.file.new-project"
  | "menu.file.settings"
  | "menu.file.close-window"
  | "menu.file.exit"
  | "menu.edit"
  | "menu.edit.undo"
  | "menu.edit.redo"
  | "menu.edit.cut"
  | "menu.edit.copy"
  | "menu.edit.paste"
  | "menu.edit.select-all"
  | "menu.edit.find"
  | "menu.edit.find-next"
  | "menu.edit.find-previous"
  | "menu.view"
  | "menu.view.reload"
  | "menu.view.actual-size"
  | "menu.view.zoom-in"
  | "menu.view.zoom-out"
  | "menu.view.copy-url"
  | "menu.help"
  | "menu.help.documentation"
  | "menu.help.check-updates"
  | "menu.help.troubleshooting"
  | "menu.help.support"
  | "menu.help.about"
  // L1 floors
  | "floor.home"
  | "floor.add"
  | "floor.rename"
  | "floor.open-in-ide"
  | "floor.pause"
  | "floor.close"
  | "floor.delete"
  // L2 sidebar tab labels
  | "nav.dashboard"
  | "nav.chat"
  | "nav.sessions"
  | "nav.inbox"
  | "nav.office"
  | "nav.employees"
  | "nav.schedule"
  | "nav.kanban"
  | "nav.automation"
  | "nav.memory"
  | "nav.wiki"
  | "nav.channels"
  | "nav.security"
  | "nav.providers"
  | "nav.settings"
  // run control
  | "run.play"
  | "run.pause"
  | "run.stop"
  // command palette
  | "palette.placeholder"
  | "palette.group.offices"
  | "palette.group.subsystems"
  | "palette.group.settings"
  | "palette.group.actions"
  | "palette.empty"
  // right file-tree dock
  | "dock.title"
  | "dock.filter.names"
  | "dock.filter.contents"
  | "dock.find-files"
  // global settings overlay
  | "settings.title"
  | "settings.back"
  | "settings.tier.global"
  | "settings.tier.local"
  | "settings.appearance"
  | "settings.appearance.mode"
  | "settings.appearance.scheme"
  | "settings.mode.system"
  | "settings.mode.light"
  | "settings.mode.dark"
  // surface placeholder
  | "surface.placeholder"
  | "frame.close-menu"
  | "floor.actions"
  | "office.state.active"
  | "office.state.idle"
  | "office.state.paused"
  | "office.state.hibernating"
  | "office.state.error"
  | "office.state.offline"
  | "sidebar.search"
  | "sidebar.help"
  | "sidebar.run-control"
  | "sidebar.run.play"
  | "sidebar.run.pause"
  | "sidebar.run.stop"
  | "status.gates-green"
  | "status.session-budget"
  | "status.weekly-budget"
  | "status.run-state"
  | "status.memory"
  | "status.run.running"
  | "status.run.paused"
  | "status.run.stopped"
  | "surface.placeholder.hint";

type Catalog = Record<MessageKey, string>;

const en: Catalog = {
  "app.title": "Cronus",
  "status.connecting": "connecting…",
  "surface.office": "Office",
  "surface.board": "Board",
  "surface.chat": "Chat",
  "surface.editor": "Editor",
  "surface.dashboard": "Dashboard",
  "surface.empty": "Nothing here yet — the core will fill this surface.",
  "office.empty": "No office staffed yet.",
  "dashboard.statistics": "Agent Statistics",
  "dashboard.building": "Building overview",
  "dashboard.offices": "Offices",
  "dashboard.active-agents": "Active agents",
  "dashboard.cards": "Cards",

  "frame.menu": "Menu",
  "frame.back": "Back",
  "frame.forward": "Forward",
  "frame.toggle-sidebar": "Toggle sidebar",
  "frame.toggle-files": "Toggle file tree",
  "frame.search": "Search",
  "frame.minimize": "Minimize",
  "frame.maximize": "Maximize",
  "frame.close": "Close",

  "menu.file": "File",
  "menu.file.open": "Open File…",
  "menu.file.new-project": "New / Add Project…",
  "menu.file.settings": "Settings…",
  "menu.file.close-window": "Close Window",
  "menu.file.exit": "Exit",
  "menu.edit": "Edit",
  "menu.edit.undo": "Undo",
  "menu.edit.redo": "Redo",
  "menu.edit.cut": "Cut",
  "menu.edit.copy": "Copy",
  "menu.edit.paste": "Paste",
  "menu.edit.select-all": "Select All",
  "menu.edit.find": "Find",
  "menu.edit.find-next": "Find Next",
  "menu.edit.find-previous": "Find Previous",
  "menu.view": "View",
  "menu.view.reload": "Reload",
  "menu.view.actual-size": "Actual Size",
  "menu.view.zoom-in": "Zoom In",
  "menu.view.zoom-out": "Zoom Out",
  "menu.view.copy-url": "Copy URL",
  "menu.help": "Help",
  "menu.help.documentation": "Open Documentation",
  "menu.help.check-updates": "Check for Updates…",
  "menu.help.troubleshooting": "Troubleshooting",
  "menu.help.support": "Get Support",
  "menu.help.about": "About…",

  "floor.home": "Home",
  "floor.add": "Add project",
  "floor.rename": "Rename…",
  "floor.open-in-ide": "Open in IDE",
  "floor.pause": "Pause",
  "floor.close": "Close tab",
  "floor.delete": "Delete workspace…",

  "nav.dashboard": "Dashboard",
  "nav.chat": "Chat",
  "nav.sessions": "Sessions",
  "nav.inbox": "Inbox",
  "nav.office": "Office",
  "nav.employees": "Employees",
  "nav.schedule": "Schedule",
  "nav.kanban": "Kanban",
  "nav.automation": "Automation",
  "nav.memory": "Memory",
  "nav.wiki": "Wiki",
  "nav.channels": "Channels",
  "nav.security": "Security",
  "nav.providers": "Providers / ACP",
  "nav.settings": "Settings",

  "run.play": "Run project",
  "run.pause": "Pause project",
  "run.stop": "Stop project",

  "palette.placeholder": "Search offices, settings, subsystems, and actions…",
  "palette.group.offices": "Recent offices",
  "palette.group.subsystems": "Go to subsystem",
  "palette.group.settings": "Settings",
  "palette.group.actions": "Actions",
  "palette.empty": "No matches.",

  "dock.title": "Files",
  "dock.filter.names": "Names",
  "dock.filter.contents": "Contents",
  "dock.find-files": "Find files",

  "settings.title": "Settings",
  "settings.back": "Back to app",
  "settings.tier.global": "Global",
  "settings.tier.local": "This workspace",
  "settings.appearance": "Appearance",
  "settings.appearance.mode": "Mode",
  "settings.appearance.scheme": "Colour scheme",
  "settings.mode.system": "System",
  "settings.mode.light": "Light",
  "settings.mode.dark": "Dark",

  "surface.placeholder": "This surface will be populated by the core.",
  "frame.close-menu": "Close menu",
  "floor.actions": "Workspace actions",
  "office.state.active": "Active",
  "office.state.idle": "Idle",
  "office.state.paused": "Paused",
  "office.state.hibernating": "Hibernating",
  "office.state.error": "Error",
  "office.state.offline": "Offline",
  "sidebar.search": "Search",
  "sidebar.help": "Help",
  "sidebar.run-control": "Project run control",
  "sidebar.run.play": "Run project",
  "sidebar.run.pause": "Pause project",
  "sidebar.run.stop": "Stop project",
  "status.gates-green": "gates green",
  "status.session-budget": "Session budget",
  "status.weekly-budget": "Weekly budget",
  "status.run-state": "Project run state",
  "status.memory": "Process Monitor",
  "status.run.running": "Running",
  "status.run.paused": "Paused",
  "status.run.stopped": "Stopped",
  "surface.placeholder.hint": "Nothing is shown until a capability is bound.",
};

// Deliberately partial: brand strings (app.title) are not translated and
// exercise the English fallback path; a handful of shell strings are also left
// to fall back, matching the existing partial-catalog contract.
const ru: Partial<Catalog> = {
  "status.connecting": "подключение…",
  "surface.office": "Офис",
  "surface.board": "Доска",
  "surface.chat": "Чат",
  "surface.editor": "Редактор",
  "surface.dashboard": "Дашборд",
  "surface.empty": "Здесь пока пусто — ядро наполнит эту поверхность.",
  "office.empty": "Офис пока не укомплектован.",
  "dashboard.statistics": "Статистика агентов",
  "dashboard.building": "Обзор здания",
  "dashboard.offices": "Офисы",
  "dashboard.active-agents": "Активные агенты",
  "dashboard.cards": "Карточки",

  "frame.menu": "Меню",
  "frame.toggle-sidebar": "Показать/скрыть боковую панель",
  "frame.toggle-files": "Показать/скрыть дерево файлов",
  "frame.search": "Поиск",

  "menu.file": "Файл",
  "menu.file.open": "Открыть файл…",
  "menu.file.new-project": "Новый / добавить проект…",
  "menu.file.settings": "Настройки…",
  "menu.file.close-window": "Закрыть окно",
  "menu.file.exit": "Выход",
  "menu.edit": "Правка",
  "menu.edit.undo": "Отменить",
  "menu.edit.redo": "Повторить",
  "menu.edit.cut": "Вырезать",
  "menu.edit.copy": "Копировать",
  "menu.edit.paste": "Вставить",
  "menu.edit.select-all": "Выделить всё",
  "menu.edit.find": "Найти",
  "menu.edit.find-next": "Найти далее",
  "menu.edit.find-previous": "Найти ранее",
  "menu.view": "Вид",
  "menu.view.reload": "Перезагрузить",
  "menu.view.actual-size": "Фактический размер",
  "menu.view.zoom-in": "Увеличить",
  "menu.view.zoom-out": "Уменьшить",
  "menu.view.copy-url": "Скопировать URL",
  "menu.help": "Справка",
  "menu.help.documentation": "Открыть документацию",
  "menu.help.check-updates": "Проверить обновления…",
  "menu.help.troubleshooting": "Диагностика",
  "menu.help.support": "Поддержка",
  "menu.help.about": "О программе…",

  "floor.home": "Главная",
  "floor.add": "Добавить проект",
  "floor.rename": "Переименовать…",
  "floor.open-in-ide": "Открыть в IDE",
  "floor.pause": "Приостановить",
  "floor.close": "Закрыть вкладку",
  "floor.delete": "Удалить воркспейс…",

  "nav.dashboard": "Дашборд",
  "nav.chat": "Чат",
  "nav.sessions": "Сессии",
  "nav.inbox": "Входящие",
  "nav.office": "Офис",
  "nav.employees": "Сотрудники",
  "nav.schedule": "Расписание",
  "nav.kanban": "Канбан",
  "nav.automation": "Автоматизация",
  "nav.memory": "Память",
  "nav.wiki": "Вики",
  "nav.channels": "Каналы",
  "nav.security": "Безопасность",
  "nav.providers": "Провайдеры / ACP",
  "nav.settings": "Настройки",

  "run.play": "Запустить проект",
  "run.pause": "Приостановить проект",
  "run.stop": "Остановить проект",

  "palette.placeholder": "Поиск офисов, настроек, подсистем и действий…",
  "palette.group.offices": "Недавние офисы",
  "palette.group.subsystems": "Перейти к подсистеме",
  "palette.group.settings": "Настройки",
  "palette.group.actions": "Действия",
  "palette.empty": "Ничего не найдено.",

  "dock.title": "Файлы",
  "dock.filter.names": "Имена",
  "dock.filter.contents": "Содержимое",
  "dock.find-files": "Найти файлы",

  "settings.title": "Настройки",
  "settings.back": "Назад в приложение",
  "settings.tier.global": "Глобальные",
  "settings.tier.local": "Этот воркспейс",
  "settings.appearance": "Оформление",
  "settings.appearance.mode": "Режим",
  "settings.appearance.scheme": "Цветовая схема",
  "settings.mode.system": "Системный",
  "settings.mode.light": "Светлый",
  "settings.mode.dark": "Тёмный",

  "surface.placeholder": "Эта поверхность будет наполнена ядром.",
  "frame.close-menu": "Закрыть меню",
  "floor.actions": "Действия рабочего пространства",
  "office.state.active": "Активен",
  "office.state.idle": "Простой",
  "office.state.paused": "На паузе",
  "office.state.hibernating": "Спящий",
  "office.state.error": "Ошибка",
  "office.state.offline": "Не в сети",
  "sidebar.search": "Поиск",
  "sidebar.help": "Справка",
  "sidebar.run-control": "Управление запуском проекта",
  "sidebar.run.play": "Запустить проект",
  "sidebar.run.pause": "Приостановить проект",
  "sidebar.run.stop": "Остановить проект",
  "status.gates-green": "проверки зелёные",
  "status.session-budget": "Бюджет сессии",
  "status.weekly-budget": "Недельный бюджет",
  "status.run-state": "Состояние запуска проекта",
  "status.memory": "Монитор процессов",
  "status.run.running": "Работает",
  "status.run.paused": "Пауза",
  "status.run.stopped": "Остановлен",
  "surface.placeholder.hint": "Ничего не отображается, пока не привязана возможность.",
};

const catalogs: Record<Locale, Partial<Catalog>> = {
  en,
  ru,
};

/** Resolve a message for a locale, falling back to English. */
export function t(locale: Locale, key: MessageKey): string {
  return catalogs[locale][key] ?? en[key];
}

/** Bind a locale once and translate with the shorter `msg(key)` form. */
export function translator(locale: Locale): (key: MessageKey) => string {
  return (key) => t(locale, key);
}
