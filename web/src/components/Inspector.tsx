import { memo } from "react";
import { nodeName } from "../state";
import type { DesignNode } from "../types";
import { Icon, type IconName } from "./Icon";
import { useNodeActions, useNodeActionBusy } from "./NodeActionsProvider";

/** Frame-local computed geometry the Inspector shows for the selected node. */
export interface ComputedNode {
  bounds: { x: number; y: number; width: number; height: number };
  kind: string;
  lines: number;
}

function Property({ name, value }: { name: string; value: unknown }) {
  const display =
    typeof value === "object" ? JSON.stringify(value, null, 2) : String(value);
  return (
    <div className="property">
      <dt>{name}</dt>
      <dd title={display}>{display}</dd>
    </div>
  );
}

export const Inspector = memo(function Inspector({
  node,
  computed,
  source,
  revision,
}: {
  node?: DesignNode;
  /** Computed geometry of `node`, only from a scene current at `revision`. */
  computed?: ComputedNode;
  /** Pipeline identity of that scene. */
  source?: { rendererVersion: string; coordinateSpace: string };
  revision?: number;
}) {
  const actions = useNodeActions();
  const busy = useNodeActionBusy();
  return (
    <aside className="inspector" aria-label="Read-only inspector">
      <div className="section-heading">
        Inspect<span className="readonly-label">Read only</span>
      </div>
      {node ? (
        <>
          <div className="inspector-identity">
            <span className="node-icon">
              <Icon name={node.kind as IconName} size={20} />
            </span>
            <div>
              <h2>{nodeName(node)}</h2>
              <span className="subtle">{node.kind}</span>
            </div>
          </div>
          {actions && (
            <div className="node-actions" aria-label="Element actions">
              <button onClick={() => actions.copy()}>Copy link</button>
              <button
                disabled={busy || node.kind === "page"}
                onClick={() => actions.image("png")}
              >
                Export PNG
              </button>
              <button
                disabled={busy || node.kind === "page"}
                onClick={() => actions.image("svg")}
              >
                Export SVG
              </button>
            </div>
          )}
          <section className="inspect-section">
            <h3>Authored properties</h3>
            <dl>
              <Property name="ID" value={node.id} />
              {Object.entries(node.props).map(([name, value]) => (
                <Property key={name} name={name} value={value} />
              ))}
            </dl>
          </section>
          <section className="inspect-section">
            <h3>Computed bounds</h3>
            {computed ? (
              <>
                <p className="section-note">
                  Frame-local pixels · revision {revision}
                </p>
                <dl className="bounds-grid">
                  {Object.entries(computed.bounds).map(([name, value]) => (
                    <Property
                      key={name}
                      name={name}
                      value={Math.round(value * 100) / 100}
                    />
                  ))}
                </dl>
                {computed.kind === "text" && (
                  <p className="section-note">
                    {computed.lines} precomputed text{" "}
                    {computed.lines === 1 ? "line" : "lines"}
                  </p>
                )}
              </>
            ) : (
              <p className="section-note">
                Available when this frame's scene is ready.
              </p>
            )}
          </section>
          {source && (
            <section className="inspect-section">
              <h3>Scene source</h3>
              <dl>
                <Property name="Revision" value={revision} />
                <Property
                  name="Renderer"
                  value={source.rendererVersion}
                />
                <Property name="Coordinates" value={source.coordinateSpace} />
              </dl>
            </section>
          )}
        </>
      ) : (
        <p className="panel-empty">
          Select a layer on the canvas or in the layers panel to inspect it.
        </p>
      )}
    </aside>
  );
});
