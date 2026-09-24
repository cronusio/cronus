/**
 * Wire a core push channel to a projection store: opening the channel
 * puts the store in `pending`, a message fulfils it, and a close — whether the
 * channel failed to open or the host reported it closed — makes it *unavailable*
 * with the reason, never leaving a stale value on screen as if current.
 *
 * There is no retry here. Reconnection is the caller re-invoking this, which
 * re-requests: a fresh `pending`, then the next `fulfill`. A gap of unknown
 * length is not reconciled by the next delta, so a mid-stream resume is wrong by
 * construction.
 */

import type { CoreClient } from "./bridge";
import type { ProjectionStore } from "./projection";

export function bindProjectionChannel<T>(
  client: CoreClient,
  channel: string,
  store: ProjectionStore<T>,
): () => void {
  store.dispatch({
    type: "request",
  });
  return client.subscribe<T>(
    channel,
    (data) =>
      store.dispatch({
        type: "fulfill",
        data,
      }),
    (reason) =>
      store.dispatch({
        type: "fail",
        reason,
      }),
  );
}
