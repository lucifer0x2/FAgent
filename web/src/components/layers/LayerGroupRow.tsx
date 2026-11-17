import { memo } from "react";
import { sectionAccent } from "../../engine/sections";
import type { BoardSection } from "../../scene/pageBoard";
import { Icon } from "../Icon";

/** All pages board: the collapsible heading of one page, absolutely placed at `top`. */
export const LayerGroupRow = memo(function LayerGroupRow({
  group,
  order,
  open,
  top,
  toggle,
}: {
  group: BoardSection;
  order: number;
  open: boolean;
  top: number;
  toggle: (group: BoardSection, open: boolean) => void;
}) {
  return (
    <li className="layer-item layer-group" style={{ top }} data-row-key={`group:${group.id}`}>
      <button
        className={`layer-group-heading ${order === 0 ? "is-first" : ""}`}
        aria-expanded={open}
        onClick={() => toggle(group, open)}
      >
        <span className={`disclosure ${open ? "is-open" : ""}`}>
          <Icon name="chevron" size={12} />
        </span>
        <span className="layer-group-dot" style={{ background: sectionAccent(order) }} />
        <span className="layer-group-name">{group.name}</span>
        <small>{group.frameIds.length}</small>
      </button>
    </li>
  );
});
