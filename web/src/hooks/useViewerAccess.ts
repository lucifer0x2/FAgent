import { useCallback } from "react";
import useSWR, { useSWRConfig } from "swr";
import { ApiError, read } from "../api";
import { overscanRatio } from "../engine/overscan";

/**
 * `GET /api/v1/viewer`: what this listener permits. Older servers omit `liveUpdates` and
 * `overscan` (the engine's overscan ratio per side, `FAGENT_VIEWER_OVERSCAN`).
 */
interface ViewerCapabilities {
  readOnly: boolean;
  liveUpdates?: boolean;
  overscan?: number;
}

const VIEWER_CAPABILITIES = "/api/v1/viewer";
const PROBE_RETRIES = 4;
const PROBE_RETRY_MS = 1000;

/**
 * A transient probe failure must not pin the page to a guess for the whole session: retry with
 * backoff (1 s, 2 s, 4 s, 8 s). A 4xx answer is the listener's decision, so it is not retried.
 */
export function probeRetryDelay(error: unknown, retryCount: number) {
  if (error instanceof ApiError && error.status && error.status < 500)
    return null;
  if (retryCount > PROBE_RETRIES) return null;
  return PROBE_RETRY_MS * 2 ** (retryCount - 1);
}

/** Listener capability is shared within the current viewer session's cache. */
function useViewerCapabilities() {
  return useSWR(
    VIEWER_CAPABILITIES,
    (path) => read<ViewerCapabilities>(path),
    {
      revalidateIfStale: false,
      revalidateOnFocus: false,
      revalidateOnReconnect: false,
      onErrorRetry: (error, _key, _config, revalidate, { retryCount }) => {
        const delay = probeRetryDelay(error, retryCount);
        if (delay !== null) setTimeout(() => revalidate({ retryCount }), delay);
      },
    },
  );
}

/** Read-only unless the listener says otherwise; a failed probe fails closed. Null while pending. */
export function useViewerAccess() {
  const { data, error } = useViewerCapabilities();
  return error ? true : data ? data.readOnly !== false : null;
}

/**
 * Whether this listener serves live event streams. A public listener may turn them off; the
 * page then shows the revision current when it loaded. A failed probe keeps today's behaviour
 * (the stream reports its own failure) while the probe retries. Null while pending, so no stream
 * opens before it is known.
 */
export function useViewerLiveUpdates() {
  const { data, error } = useViewerCapabilities();
  return error ? true : data ? data.liveUpdates !== false : null;
}

/**
 * Overscan ratio per side for the Vello engine: 0 (none) while pending, after a failed
 * probe, from an older server that omits it, or for an out-of-range value.
 */
export function useViewerOverscan() {
  const { data } = useViewerCapabilities();
  return overscanRatio(data?.overscan);
}

/**
 * Re-reads the capability probe. A stream the listener denies (403 `PUBLIC_LIVE_DISABLED` when
 * live updates are off) calls this, so a page that guessed "live" after a failed probe settles on
 * the static status instead of reconnecting forever. `EventSource` hides the status code, so the
 * probe is the authority.
 */
export function useViewerCapabilityRecheck() {
  const { mutate } = useSWRConfig();
  return useCallback(() => void mutate(VIEWER_CAPABILITIES), [mutate]);
}
