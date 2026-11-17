import { createRoot } from "react-dom/client";
import { SWRConfig } from "swr";
import { useProjectStream } from "../src/hooks/useProjectStream";
import {
  useViewerCapabilityRecheck,
  useViewerLiveUpdates,
} from "../src/hooks/useViewerAccess";

const ignore = () => {};

/** The same wiring as useProjectLive: a denied stream re-reads the capability probe. */
function Live() {
  const live = useViewerLiveUpdates();
  const recheck = useViewerCapabilityRecheck();
  const stream = useProjectStream(
    "",
    0,
    {
      cursor: () => 0,
      onOpen: ignore,
      onCommit: ignore,
      onReset: ignore,
      onVisibilityChanged: ignore,
      onReconnect: ignore,
      onCreated: ignore,
      onActivity: ignore,
      onDenied: recheck,
    },
    live === true,
  );
  return (
    <output aria-label="connection">
      {live === null ? "pending" : live === false ? "static" : stream}
    </output>
  );
}

createRoot(document.getElementById("root")!).render(
  <SWRConfig value={{ provider: () => new Map() }}>
    <Live />
  </SWRConfig>,
);
