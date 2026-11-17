import { FollowToggle } from "../components/FollowToggle";
import type { SceneView } from "../hooks/useScene";
import type { ProjectView } from "./projectView";
import { WASM_FALLBACK } from "../scene/service";
import { ENGINE_FALLBACK } from "../engine/client";
import { shownRevision } from "../scene/staleFrames";

/** Connection, authored/scene provenance and opt-in resource notices. */
export function ViewerStatus({
  project,
  view,
  showNotice,
}: {
  project: ProjectView;
  view: SceneView;
  showNotice: boolean;
}) {
  const { snapshot, selection } = project;
  const fallbackNotice = view.diagnostics.find((message) =>
    message.startsWith(WASM_FALLBACK) || message.startsWith(ENGINE_FALLBACK),
  );
  const sceneStatus =
    view.status === "pending"
      ? view.progress
        ? `${view.progress.ready}/${view.progress.total} frames ready`
        : "Scene pending"
      : view.status === "failed"
        ? "Render failed"
        : view.status === "ready"
          ? `Scene r${shownRevision(view)}`
          : "No scene";
  const notice =
    project.error ||
    fallbackNotice ||
    view.missing.join(" · ") ||
    view.diagnostics.join(" · ") ||
    project.notice;

  return (
    <>
      {notice && (showNotice || fallbackNotice) ? (
        <div
          className={`notice-bar ${project.error || view.missing.length ? "warning" : ""}`}
          role="status"
        >
          <span>{notice}</span>
          {project.error && (
            <button onClick={project.retry}>Retry connection</button>
          )}
        </div>
      ) : null}
      <footer className="statusbar">
        <span
          className={`connection ${project.connection}`}
          title={
            project.connection === "static"
              ? "Live updates are off for this viewer. Reload the page to see newer changes."
              : undefined
          }
        >
          <span className="status-dot" />
          {selection.projectId
            ? project.connection === "live"
              ? "Live"
              : project.connection === "static"
                ? "Reload for latest"
                : project.connection === "reconnecting"
                  ? "Reconnecting…"
                  : "Connecting…"
            : "Local viewer"}
        </span>
        {/* Following the agent needs live hints. */}
        {project.connection !== "static" && (
          <FollowToggle follow={project.follow} change={project.setFollow} />
        )}
        <div>
          <span>
            {snapshot
              ? `Authored r${snapshot.revision}`
              : project.error
                ? "Snapshot unavailable"
                : selection.projectId
                  ? "Loading snapshot…"
                  : "No project selected"}
          </span>
          <span className={view.status === "failed" ? "error-text" : ""}>
            {sceneStatus}
          </span>
          {view.engine && (
            <span className="renderer-backend" title="Vello engine backend">
              {view.backend ? `Vello · ${view.backend}` : "Vello · starting"}
            </span>
          )}
        </div>
      </footer>
    </>
  );
}
