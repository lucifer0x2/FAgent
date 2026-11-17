import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { SWRConfig } from "swr";
import {
  useViewerAccess,
  useViewerLiveUpdates,
} from "../src/hooks/useViewerAccess";

function Access({ name }: { name: string }) {
  const access = useViewerAccess();
  const live = useViewerLiveUpdates();
  return (
    <>
      <output aria-label={name}>{access === null ? "pending" : access ? "read only" : "writable"}</output>
      <output aria-label={`${name} live`}>{live === null ? "pending" : live ? "live" : "static"}</output>
    </>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode><SWRConfig value={{ provider: () => new Map() }}>
    <Access name="first" /><Access name="second" />
  </SWRConfig></StrictMode>,
);
