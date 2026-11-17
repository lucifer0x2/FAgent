import { ALL_FRAMES, ALL_PAGES } from "../frameScope";
import { nodeName } from "../state";
import type { DesignNode, Selection } from "../types";
import { CustomSelect } from "./CustomSelect";
import { Icon } from "./Icon";

export const PRESETS = [
  { key: "frame", name: "Frame size", width: 0, height: 0 },
  { key: "desktop", name: "Desktop", width: 1440, height: 900 },
  { key: "tablet", name: "Tablet", width: 768, height: 1024 },
  { key: "mobile", name: "Mobile", width: 390, height: 844 },
];

export function ScopeControls({
  pages,
  frames,
  selection,
  select,
  allPages = false,
  preset,
  setPreset,
}: {
  pages: DesignNode[];
  frames: DesignNode[];
  selection: Selection;
  select: (patch: Partial<Selection>) => void;
  /** Offer the All pages board (Vello engine only). */
  allPages?: boolean;
  preset: string;
  setPreset: (value: string) => void;
}) {
  const everyPage = selection.pageId === ALL_PAGES;
  const pageName = new Map(pages.map((page) => [page.id, nodeName(page)]));
  return (
    <div className="viewbar">
      <div className="scope-controls">
        <label>
          <span>Page</span>
          <CustomSelect
            label="Page"
            disabled={!pages.length}
            value={selection.pageId}
            placeholder="No pages"
            options={[
              ...(allPages && pages.length
                ? [{ value: ALL_PAGES, label: "All", description: `${pages.length} ${pages.length === 1 ? "page" : "pages"}` }]
                : []),
              ...pages.map((page) => ({
                value: page.id,
                label: nodeName(page),
              })),
            ]}
            onChange={(pageId) =>
              select({
                pageId,
                frameId: ALL_FRAMES,
                nodeId: "",
              })
            }
          />
        </label>
        <span className="scope-slash">/</span>
        <label>
          <span>Frame</span>
          <CustomSelect
            label="Frame"
            disabled={!pages.length}
            value={selection.frameId}
            placeholder="No frames"
            options={[
              { value: ALL_FRAMES, label: "All" },
              ...frames.map((frame) => ({
                value: frame.id,
                label: nodeName(frame),
                description: everyPage ? pageName.get(frame.parentId ?? "") : undefined,
              })),
            ]}
            onChange={(frameId) =>
              everyPage && frameId !== ALL_FRAMES
                ? // A frame of the All pages board opens on its own page.
                  select({
                    pageId: frames.find((frame) => frame.id === frameId)?.parentId ?? "",
                    frameId,
                    nodeId: frameId,
                  })
                : select({ frameId, nodeId: frameId === ALL_FRAMES ? "" : frameId })
            }
          />
        </label>
      </div>
      <label className="viewport-picker">
        <Icon name="frame" />
        <span className="sr-only">Viewport</span>
        <CustomSelect
          label="Viewport"
          value={preset}
          disabled={!frames.length}
          onChange={setPreset}
          options={PRESETS.map((entry) => ({
            value: entry.key,
            label: entry.name,
            description: entry.width
              ? `${entry.width} × ${entry.height}`
              : undefined,
          }))}
        />
      </label>
    </div>
  );
}
