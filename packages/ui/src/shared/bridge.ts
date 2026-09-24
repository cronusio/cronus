/**
 * Typed client for the shell ↔ core IPC bridge.
 *
 * Pure marshalling: each method forwards to one shell IPC command that binds a
 * core capability (the same surface the CLI/TUI bind), or opens one push channel
 * the core emits on. The `invoke` and `listen` functions are injected by the
 * hosting shell, so this package stays shell-agnostic and testable without a
 * Tauri runtime. No business logic lives here.
 */

/** Shape of the shell's IPC invoke function (injected by the host app). */
export type InvokeFn = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;

/**
 * Shape of the host's event-listen function (injected alongside `invoke`).
 * Resolves to a function that detaches the listener; rejects if the channel
 * cannot be opened.
 */
export type ListenFn = <T>(
  channel: string,
  handler: (event: { payload: T }) => void,
) => Promise<() => void>;

/**
 * What the core emits on a push channel: a message, or a one-shot close the
 * host frames itself. After `closed` no further messages arrive on that
 * subscription — the caller re-subscribes to resume, which is a fresh request,
 * never a mid-stream continuation.
 */
export type ChannelEvent<T> =
  | {
      type: "message";
      data: T;
    }
  | {
      type: "closed";
      reason: string;
    };

/**
 * The shell-facing slice of the host's settings store. Host-owned configuration
 * the shell reads and writes as marshalling, not logic (admission rule:
 * a host-owned facility). `layout` is opaque here: the frontend owns the
 * `LayoutRecord` schema and its field-wise restore.
 */
export interface ShellSettings {
  theme: string;
  colorScheme: string;
  layout: unknown;
  keymapUser: Record<string, string>;
}

/**
 * Where an invocable's behavior is decided — mirrors
 * `cronus_contract::Locus`'s wire shape one-to-one (a unit variant is a bare
 * string under serde's default external tagging; `HostOnly` is the one
 * variant carrying a field). `capability_catalog` never actually returns a
 * `HostOnly`/`Installation` entry (the core filters to `Semantic`+
 * `ClientLocal` before serializing), but the type stays complete rather than
 * silently narrower than what the Rust type can express.
 */
export type Locus =
  | "Semantic"
  | "ClientLocal"
  | "Installation"
  | {
      HostOnly: {
        reason: string;
      };
    };

/**
 * Whether an invocable is on the shipped surface — mirrors
 * `cronus_contract::Stability`. `capability_catalog` only ever returns
 * `"Shipped"` entries in practice (the same filter `Locus` above notes), but
 * `Retired` stays representable for the same reason.
 */
export type Stability =
  | "Shipped"
  | {
      Retired: {
        superseded_by: string;
      };
    };

/** The closed set of shapes a [`Binder`] may declare — mirrors `cronus_contract::BinderKind`, a fieldless enum (bare strings under external tagging). */
export type BinderKind =
  | "Text"
  | "Integer"
  | "Boolean"
  | "Flag"
  | "NamedText"
  | "Float"
  | "RepeatableNamedText";

/** One argument an invocable declares, in order — mirrors `cronus_contract::Binder`. */
export interface Binder {
  name: string;
  kind: BinderKind;
  optional: boolean;
}

/**
 * A frontend-projectable action descriptor (mirrors `cronus_contract::Invocable`) —
 * data-only by construction: every field is a string, a boolean, or
 * one of the closed unions above, so there is no field through which a host
 * handle or a closure could travel. That guarantee is a property of this
 * type, not a runtime check.
 *
 * Deliberately narrower than the Rust struct: `journal_raw_input` is
 * dispatch-journal bookkeeping this client has no use for, so it is left
 * off rather than mirrored for completeness's own sake.
 */
export interface Invocable {
  id: string;
  name: string;
  summary: string;
  group: string;
  locus: Locus;
  binders: Binder[];
  stability: Stability;
}

/**
 * One argument value, shaped like the [`BinderKind`] it binds — mirrors
 * `cronus_contract::ArgValue`'s wire shape (the JS → Rust half of the seam;
 * `Flag` is the one variant with no payload, a bare string under external
 * tagging).
 */
export type ArgValue =
  | {
      Text: string;
    }
  | {
      Integer: number;
    }
  | {
      Boolean: boolean;
    }
  | "Flag"
  | {
      Float: number;
    }
  | {
      List: string[];
    };

/**
 * What the caller supplies to [`CoreClient.invoke`]. Deliberately just
 * `id`+`args`, not a mirror of the Rust `Invocation` struct: the caller
 * identity (`Surface::Desktop`) is asserted by the bridge itself, never
 * accepted from this seam (the same reasoning extended from payload
 * shape to identity) — this executable face has no field through which to
 * claim a different surface even if it tried.
 */
export interface Invocation {
  id: string;
  args?: Record<string, ArgValue>;
}

/**
 * A bounded structured value tree — mirrors `cronus_contract::OutcomeValue`.
 * `Empty` is a bare string (a unit variant); `Record`'s entries are
 * `[key, value]` pairs, the JSON shape a Rust `(String, OutcomeValue)` tuple
 * serializes to.
 */
export type OutcomeValue =
  | "Empty"
  | {
      Text: string;
    }
  | {
      Integer: number;
    }
  | {
      Boolean: boolean;
    }
  | {
      List: OutcomeValue[];
    }
  | {
      Record: Array<
        [
          string,
          OutcomeValue,
        ]
      >;
    };

/** The closed set of reasons a binder failed to produce a value — mirrors `cronus_contract::RejectionMode`. */
export type RejectionMode = "Absent" | "Unreadable" | "Malformed" | "IllShaped";

/** A binding failure, naming its mode and the binder it concerns — mirrors `cronus_contract::Rejection`. */
export interface Rejection {
  binder: string;
  mode: RejectionMode;
  detail: string;
}

/** A handle to the push-channel transport ([`CoreClient.subscribe`]) — mirrors `cronus_contract::StreamHandle`. */
export interface StreamHandle {
  channel: string;
}

/**
 * What [`CoreClient.invoke`] resolves to for a **resolved** invocation
 * (mirrors `cronus_contract::Outcome`) — data-only, the same data-only
 * guarantee [`Invocable`] carries. An invocation naming nothing the
 * registry knows resolves to `null` instead of any `Outcome` shape — see
 * [`CoreClient.invoke`]'s own doc comment.
 */
export type Outcome =
  | {
      Value: OutcomeValue;
    }
  | {
      Stream: StreamHandle;
    }
  | {
      Rejected: Rejection;
    }
  | {
      Unavailable: {
        reason: string;
      };
    };

/** Typed view of the core capability surface exposed over IPC. */
export interface CoreClient {
  /** Core/product version string. */
  version(): Promise<string>;
  /** Human-readable core status line (already masked by the core). */
  status(): Promise<string>;
  /**
   * Every descriptor this shell may project, sourced from the
   * shared registry — never a hand-written list local to this client.
   */
  catalog(): Promise<Invocable[]>;
  /**
   * Dispatch one call through the shared registry/dispatcher boundary.
   * `null` is the registry's real "unknown" answer — a stale local
   * catalog copy naming an id the core no longer recognises, never a
   * fabricated failure. This method only marshals that answer through;
   * deciding to refresh the local catalog on it is the caller's own concern.
   */
  invoke(invocation: Invocation): Promise<Outcome | null>;
  /**
   * Open a push channel. `onMessage` gets each payload; `onClose` fires
   * exactly once — if the channel fails to open, or the host reports it closed —
   * after which the subscription is dead. Returns a function that detaches it.
   * Never retries: reconnection follows the host's connection lifecycle,
   * not a frontend timer.
   */
  subscribe<T>(
    channel: string,
    onMessage: (payload: T) => void,
    onClose?: (reason: string) => void,
  ): () => void;
  /** Host-owned settings the shell persists through. */
  settings: {
    /** Read the current shell-facing settings slice. */
    get(): Promise<ShellSettings>;
    /** Write a partial update; only the given fields change. */
    set(patch: Partial<ShellSettings>): Promise<void>;
  };
}

/** Wrap a shell invoke (and optional listen) function into the typed core client. */
export function createCoreClient(invoke: InvokeFn, listen?: ListenFn): CoreClient {
  return {
    version: () => invoke<string>("capability_version"),
    status: () => invoke<string>("capability_status"),
    catalog: () => invoke<Invocable[]>("capability_catalog"),
    invoke: (invocation) =>
      invoke<Outcome | null>("capability_invoke", {
        id: invocation.id,
        args: invocation.args ?? {},
      }),
    settings: {
      get: () => invoke<ShellSettings>("capability_settings_get"),
      set: (patch) =>
        invoke<void>("capability_settings_set", {
          patch,
        }),
    },
    subscribe: <T>(
      channel: string,
      onMessage: (payload: T) => void,
      onClose?: (reason: string) => void,
    ) => {
      let live = true;
      let detach: (() => void) | null = null;

      const close = (reason: string) => {
        if (!live) {
          return;
        }
        live = false;
        detach?.();
        detach = null;
        onClose?.(reason);
      };

      if (!listen) {
        // No event transport on this host — the channel cannot open.
        queueMicrotask(() => close("no event transport"));
        return () => {
          live = false;
        };
      }

      listen<ChannelEvent<T>>(channel, ({ payload }) => {
        if (!live) {
          return;
        }
        if (payload.type === "closed") {
          close(payload.reason);
        } else {
          onMessage(payload.data);
        }
      }).then(
        (unlisten) => {
          if (live) {
            detach = unlisten;
          } else {
            unlisten();
          }
        },
        (error: unknown) => {
          close(error instanceof Error ? error.message : "channel failed to open");
        },
      );

      return () => {
        live = false;
        detach?.();
        detach = null;
      };
    },
  };
}
