import {
  FRAME_LABEL,
  packFrames,
  REFERENCE_ASPECT,
  REFERENCE_VIEW,
  type FramePlacement,
  type FrameSize,
} from "./frameLayout.ts";

/** One page of the All pages board, its frames in authored order. */
export interface BoardSection {
  id: string;
  name: string;
  frameIds: string[];
}

/**
 * A page's area on the board (board units). Its title tab sits in the space reserved above
 * it; `focus` is the area (with that title) a click on the title zooms to.
 */
export interface SectionPlacement {
  id: string;
  name: string;
  index: number;
  frames: number;
  x: number;
  y: number;
  width: number;
  height: number;
  focus: { x: number; y: number; width: number; height: number };
}

export interface PageBoardLayout {
  frames: FramePlacement[];
  sections: SectionPlacement[];
  width: number;
  height: number;
}

// Minimum board units; each grows with its page so that, zoomed to fit that page, the
// padding and frame labels have about the same screen size on every page.
const PAD = 48;
const GAP = 160;
const TITLE = 48;
const EMPTY = { width: 320, height: 120 };
// Screen px. `title`: room above a page for its title tab (24 px tab + spacing).
const SCREEN = { pad: 16, label: 36, gap: 32, title: 36 };

/**
 * Every page as a titled area: frames packed per page as on a page board, then the pages
 * packed as blocks (skyline, like frames) with their titles above them. Room is sized for
 * the reference view (`REFERENCE_VIEW`), so positions depend only on the content: they do
 * not change with the pane size, the drawers or live zoom (as `fitFrameBoard`).
 */
export function fitPageBoard(
  frames: FrameSize[],
  sections: BoardSection[],
): PageBoardLayout {
  const { width: viewWidth, height: viewHeight } = REFERENCE_VIEW;
  const aspect = REFERENCE_ASPECT;
  const byId = new Map(frames.map((frame) => [frame.id, frame]));
  const blocks = sections.map((section) => {
    const own = section.frameIds.flatMap((id) => byId.get(id) ?? []);
    const first = packFrames(own, aspect);
    // Board units per screen px when this page alone is fitted.
    const unit =
      Math.max(first.width / aspect, first.height, EMPTY.height) / viewHeight;
    const inner = packFrames(own, aspect, Math.max(FRAME_LABEL, SCREEN.label * unit));
    const pad = Math.max(PAD, SCREEN.pad * unit);
    return {
      section,
      inner,
      unit,
      pad,
      count: own.length,
      width: Math.max(inner.width, EMPTY.width) + 2 * pad,
      height: (own.length ? inner.height : EMPTY.height) + 2 * pad,
    };
  });
  const mean = blocks.reduce((sum, block) => sum + block.unit, 0) / (blocks.length || 1);
  const gap = Math.max(GAP, SCREEN.gap * mean);
  const sizes = blocks.map((block, index) => ({
    id: String(index),
    width: block.width,
    height: block.height,
  }));
  // The title room above each row must read at the whole board's Fit scale.
  let scale = 1;
  let title = TITLE;
  let outer = packFrames(sizes, aspect, title, gap);
  for (let i = 0; i < 16 && sizes.length; i++) {
    const fitted = Math.max(
      0.0001,
      Math.min(viewWidth / outer.width, viewHeight / outer.height, 1),
    );
    if (fitted >= scale * 0.99) break;
    // Only shrinking: the room grows monotonically, so the search cannot oscillate.
    scale = fitted;
    title = Math.max(TITLE, SCREEN.title / scale);
    outer = packFrames(sizes, aspect, title, gap);
  }
  const placed: FramePlacement[] = [];
  const placedSections = outer.frames.map((at): SectionPlacement => {
    const index = Number(at.id);
    const block = blocks[index];
    const x = at.x + block.pad + (block.width - 2 * block.pad - block.inner.width) / 2;
    const y = at.y + block.pad;
    // A label may use the page's padding, never the title room above the page.
    for (const frame of block.inner.frames)
      placed.push({
        ...frame,
        x: x + frame.x,
        y: y + frame.y,
        labelSpace: Math.min(frame.labelSpace ?? FRAME_LABEL, block.pad + frame.y),
      });
    const above = SCREEN.title * block.unit;
    return {
      id: block.section.id,
      name: block.section.name,
      index,
      frames: block.count,
      x: at.x,
      y: at.y,
      width: block.width,
      height: block.height,
      focus: { x: at.x, y: at.y - above, width: block.width, height: block.height + above },
    };
  });
  // The first row's title room is part of the board, so Fit shows it.
  return {
    frames: placed,
    sections: placedSections,
    width: outer.width,
    height: outer.height,
  };
}
