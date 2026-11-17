import { useCallback, useState } from "react";
import type { BoardSection } from "../../scene/pageBoard.ts";

/**
 * Expanded/collapsed state for node rows and page groups, held above the rows so it survives
 * rows scrolling out of the virtual window.
 *
 * - Node rows start expanded; `collapsed` lists those the user closed.
 * - Pages start collapsed (a whole project's tree is large); `toggled` records manual choices.
 * - A new selection opens its ancestors and its page, even if they were closed by hand, and
 *   the user can close them again afterwards.
 */
export function useLayerExpansion(
  groups: readonly BoardSection[] | undefined,
  pathIds: readonly string[],
) {
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(() => new Set());
  const [toggled, setToggled] = useState<ReadonlyMap<string, boolean>>(() => new Map());
  const selectedRoot = pathIds[0];
  const pathKey = pathIds.join("\n");

  // Adjust during render (not in an effect) so the rows, the window and the reveal of a new
  // selection all come from one consistent commit.
  const [seenPath, setSeenPath] = useState<string>();
  if (seenPath !== pathKey) {
    setSeenPath(pathKey);
    const ancestors = pathIds.slice(0, -1);
    if (ancestors.some((id) => collapsed.has(id))) {
      const next = new Set(collapsed);
      for (const id of ancestors) next.delete(id);
      setCollapsed(next);
    }
    const group = groups?.find(
      (entry) => selectedRoot && entry.frameIds.includes(selectedRoot),
    );
    if (group && toggled.has(group.id)) {
      const next = new Map(toggled);
      next.delete(group.id);
      setToggled(next);
    }
  }

  const isGroupOpen = useCallback(
    (group: BoardSection) =>
      toggled.get(group.id) ?? (!!selectedRoot && group.frameIds.includes(selectedRoot)),
    [toggled, selectedRoot],
  );

  const toggleGroup = useCallback(
    (group: BoardSection, open: boolean) =>
      setToggled((old) => new Map(old).set(group.id, !open)),
    [],
  );

  const toggleNode = useCallback(
    (id: string) =>
      setCollapsed((old) => {
        const next = new Set(old);
        if (!next.delete(id)) next.add(id);
        return next;
      }),
    [],
  );

  return { collapsed, isGroupOpen, toggleGroup, toggleNode };
}
