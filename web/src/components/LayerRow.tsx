import { memo } from "react";
import { nodeName } from "../state";
import type { DesignNode } from "../types";
import { Icon, type IconName } from "./Icon";

/** One node line of the virtualized Layers list, absolutely placed at `top`. */
export const LayerRow = memo(function LayerRow({
  node,
  depth,
  top,
  expandable,
  expanded,
  selected,
  select,
  toggle,
  openActions,
}: {
  node: DesignNode;
  depth: number;
  top: number;
  expandable: boolean;
  expanded: boolean;
  selected: boolean;
  select: (id: string) => void;
  toggle: (id: string) => void;
  openActions?: (id: string, x: number, y: number) => void;
}) {
  return (
    <li className="layer-item" style={{ top }} data-row-key={node.id}>
      <div
        className={`layer-row ${selected ? "is-selected" : ""}`}
        style={{ paddingLeft: 10 + depth * 14 }}
        onContextMenu={(event) => {
          if (!openActions) return;
          event.preventDefault();
          openActions(node.id, event.clientX, event.clientY);
        }}
      >
        {expandable ? (
          <button
            className={`disclosure ${expanded ? "is-open" : ""}`}
            aria-label={`${expanded ? "Collapse" : "Expand"} ${nodeName(node)}`}
            aria-expanded={expanded}
            onClick={() => toggle(node.id)}
          >
            <Icon name="chevron" size={12} />
          </button>
        ) : (
          <span className="disclosure-spacer" />
        )}
        <button
          className="layer-select"
          onClick={() => select(node.id)}
          onKeyDown={(event) => {
            if (
              event.key === "ContextMenu" ||
              (event.shiftKey && event.key === "F10")
            ) {
              event.preventDefault();
              const rect = event.currentTarget.getBoundingClientRect();
              openActions?.(node.id, rect.left, rect.bottom);
            }
          }}
          aria-pressed={selected}
          title={nodeName(node)}
        >
          <Icon name={node.kind as IconName} />
          <span>{nodeName(node)}</span>
        </button>
      </div>
    </li>
  );
});
