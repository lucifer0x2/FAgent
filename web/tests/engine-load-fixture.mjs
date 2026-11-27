// A large multi-page project for engine-load.mjs, seeded through MCP on a temporary backend:
// by default 225 root frames over 10 pages, column/row stacks, ~2.7k component instances
// (nav, button, card, footer; nested), five fonts and one image. Synthetic content only.
import { readFile } from "node:fs/promises";
import { connectMcp } from "../../spikes/e2e/protocol.mjs";
import { stripesPNG } from "./fixture-png.mjs";

const FONTS = [
  { id: "font_lato", file: "fixtures/fonts/Lato-Regular.ttf", family: "Lato", weight: 400, style: "normal" },
  { id: "font_lato_bold", file: "fixtures/fonts/Lato-Bold.ttf", family: "Lato", weight: 700, style: "normal" },
  { id: "font_lato_italic", file: "fixtures/fonts/Lato-Italic.ttf", family: "Lato", weight: 400, style: "italic" },
  { id: "font_noto", file: "renderer/fonts/NotoSans.ttf", family: "Noto Sans", weight: 400, style: "normal" },
];
const COPY = [
  "Plan, review and ship design changes with live previews, shared tokens and precise layout.",
  "Every frame keeps its own revision, so agents and people can edit the same board safely.",
  "Components, themes and variable fonts resolve on the server before any pixel is drawn.",
  "Large boards stay responsive: unchanged frames are reused and only edited ones recompute.",
];

const node = (id, kind, parentId, props) => ({ op: "create_node", id, kind, parentId, props });
const text = (id, parentId, value, fontId, fontSize, extra = {}) =>
  node(id, "text", parentId, {
    name: id, text: value, width: "fill", height: "hug", fontId, fontSize,
    lineHeight: Math.round(fontSize * 1.4), fill: "#1d2433", ...extra,
  });

/** Masters on their own page: button, card (with a nested button), nav and footer. */
function components(assetId) {
  return [
    node("components", "page", null, { name: "Components" }),
    node("button", "frame", "components", {
      name: "Button", component: { name: "Button" }, x: 0, y: 0, width: 140, height: 40,
      radius: 8, fill: "#1672e8", layout: { mode: "row", align: "center", padding: 10, gap: 8 },
    }),
    text("button-label", "button", "Get started", "font_lato_bold", 14, { fill: "#ffffff" }),
    node("card", "frame", "components", {
      name: "Card", component: { name: "Card" }, x: 200, y: 0, width: 312, height: "hug",
      radius: 12, fill: "#ffffff", stroke: { color: "#c9d3e0", width: 1 },
      shadow: [{ x: 0, y: 4, blur: 12, color: "#1d2433" }],
      layout: { mode: "column", padding: 16, gap: 10 },
    }),
    node("card-media", "image", "card", { name: "Media", width: "fill", height: 150, radius: 8, assetId, fit: "cover" }),
    text("card-title", "card", "Card title", "font_lato_bold", 18),
    text("card-body", "card", COPY[0], "font_noto", 14, { fill: "#505b6b" }),
    node("card-meta", "frame", "card", {
      name: "Meta", width: "fill", height: 40, layout: { mode: "row", align: "center", gap: 8 },
    }),
    text("card-tag", "card-meta", "Updated today", "font_lato_italic", 12, { fill: "#6b7686" }),
    node("card-cta", "instance", "card-meta", { componentId: "button" }),
    node("nav", "frame", "components", {
      name: "Nav", component: { name: "Nav" }, x: 0, y: 600, width: 1440, height: 72,
      fill: "#ffffff", layout: { mode: "row", align: "center", padding: 24, gap: 32 },
    }),
    text("nav-logo", "nav", "Fagent Studio", "font_lato_bold", 20, { width: 220 }),
    ...["Product", "Pricing", "Docs", "Blog", "Company"].map((label, i) =>
      text(`nav-link-${i}`, "nav", label, "font_lato", 15, { width: 120 })),
    node("nav-cta", "instance", "nav", { componentId: "button" }),
    node("footer", "frame", "components", {
      name: "Footer", component: { name: "Footer" }, x: 0, y: 800, width: 1440, height: "hug",
      fill: "#0f172a", layout: { mode: "column", padding: 48, gap: 12 },
    }),
    text("footer-title", "footer", "Fagent Studio", "font_lato_bold", 22, { fill: "#ffffff" }),
    text("footer-copy", "footer", COPY[1], "font_noto", 14, { fill: "#cbd5e1" }),
    text("footer-legal", "footer", "© Example Inc. All rights reserved.", "font_lato", 12, { fill: "#94a3b8" }),
  ];
}

/** One landing-page-like root frame: nav, hero, a card grid, an article and a footer. */
function screen(page, index, rows) {
  const id = `${page}-f${index}`;
  const ops = [
    node(id, "frame", page, {
      name: `Screen ${index + 1}`, width: 1440, height: "hug", fill: "#f6f8fb",
      layout: { mode: "column", gap: 0 },
    }),
    node(`${id}-nav`, "instance", id, { componentId: "nav" }),
    node(`${id}-hero`, "frame", id, {
      name: "Hero", width: "fill", height: "hug", fill: "#eef3f8",
      layout: { mode: "column", padding: 64, gap: 16 },
    }),
    text(`${id}-h1`, `${id}-hero`, `Screen ${index + 1}: ship design changes faster`, "font_lato_bold", 48),
    text(`${id}-sub`, `${id}-hero`, COPY[index % COPY.length], "font_lato", 20, { fill: "#505b6b" }),
    node(`${id}-ctas`, "frame", `${id}-hero`, {
      name: "Actions", width: "fill", height: 48, layout: { mode: "row", gap: 12 },
    }),
    node(`${id}-cta-a`, "instance", `${id}-ctas`, { componentId: "button" }),
    node(`${id}-cta-b`, "instance", `${id}-ctas`, {
      componentId: "button", overrides: { "button-label": { text: "Book a demo" } },
    }),
    node(`${id}-grid`, "frame", id, {
      name: "Grid", width: "fill", height: "hug", layout: { mode: "column", padding: 48, gap: 24 },
    }),
  ];
  for (let r = 0; r < rows; r++) {
    const row = `${id}-row${r}`;
    ops.push(node(row, "frame", `${id}-grid`, {
      name: `Row ${r}`, width: "fill", height: "hug", layout: { mode: "row", gap: 24 },
    }));
    for (let c = 0; c < 4; c++)
      ops.push(node(`${row}-c${c}`, "instance", row, {
        componentId: "card",
        overrides: {
          "card-title": { text: `Feature ${r + 1}.${c + 1} on screen ${index + 1}` },
          "card-body": { text: COPY[(r + c + index) % COPY.length] },
        },
      }));
  }
  ops.push(
    node(`${id}-article`, "frame", id, {
      name: "Article", width: "fill", height: "hug", fill: "#ffffff",
      layout: { mode: "column", padding: 64, gap: 12 },
    }),
    ...[0, 1, 2].map((p) =>
      text(`${id}-p${p}`, `${id}-article`, `${COPY[p]} ${COPY[(p + 1) % 4]} ${COPY[(p + 2) % 4]}`, "font_noto", 16)),
    text(`${id}-quote`, `${id}-article`, COPY[3], "font_lato_italic", 22, { fill: "#1672e8" }),
    node(`${id}-footer`, "instance", id, { componentId: "footer" }),
  );
  return ops;
}

/**
 * Frame counts per page summing to `frames`, uneven like a real project; every page has at
 * least one frame, so `pages` must not exceed `frames` (both positive integers).
 */
export function pageSizes(frames, pages) {
  for (const [name, value] of Object.entries({ frames, pages }))
    if (!Number.isInteger(value) || value < 1)
      throw new Error(`${name} must be a positive integer, got ${value}`);
  if (pages > frames) throw new Error(`${pages} pages need at least ${pages} frames, got ${frames}`);
  const weights = Array.from({ length: pages }, (_, i) => 1 + ((i * 7) % 5));
  const total = weights.reduce((a, b) => a + b, 0);
  const sizes = weights.map((w) => Math.max(1, Math.floor((w / total) * frames)));
  const sum = () => sizes.reduce((a, b) => a + b, 0);
  for (let i = 0; sum() < frames; i++) sizes[i % pages]++;
  // Raising empty pages to one frame can overshoot: take the excess from the largest pages.
  while (sum() > frames) sizes[sizes.indexOf(Math.max(...sizes))]--;
  return sizes;
}

/** Creates the project through MCP; resolves with its id, revision and the generated counts. */
export async function seedLargeProject(base, root, { frames = 225, pages = 10 } = {}) {
  const sizes = pageSizes(frames, pages);
  const mcp = await connectMcp(base);
  try {
    const { projectId } = await mcp.call("create_project", { slug: "engine-load", name: "Engine load fixture" });
    let revision = 0;
    for (const font of FONTS) {
      const result = await mcp.call("import_asset", {
        projectId, expectedRevision: revision, idempotencyKey: font.id, mediaType: "font/ttf",
        base64: (await readFile(new URL(font.file, root))).toString("base64"),
        font: { id: font.id, family: font.family, weight: font.weight, style: font.style },
      });
      revision = result.committedRevision;
    }
    const image = await mcp.call("import_asset", {
      projectId, expectedRevision: revision, idempotencyKey: "image", mediaType: "image/png",
      base64: stripesPNG(320, 120).toString("base64"),
    });
    revision = image.committedRevision;
    const ops = components(image.asset.id);
    let instances = ops.filter((op) => op.kind === "instance").length;
    sizes.forEach((count, p) => {
      const page = `pg-${p}`;
      ops.push(node(page, "page", null, { name: `Page ${p + 1}` }));
      for (let f = 0; f < count; f++) {
        const frameOps = screen(page, f, 2 + ((f + p) % 3));
        instances += frameOps.filter((op) => op.kind === "instance").length;
        ops.push(...frameOps);
      }
    });
    for (let offset = 0; offset < ops.length; offset += 500) {
      const result = await mcp.call("apply_batch", {
        projectId, expectedRevision: revision, idempotencyKey: `seed-${offset}`,
        operations: ops.slice(offset, offset + 500),
      });
      revision = result.committedRevision;
    }
    // Counted from the generated operations, so the benchmark waits for what exists.
    const screens = ops.filter((op) => op.kind === "frame" && /^pg-\d+$/.test(op.parentId)).length;
    return {
      projectId, revision, nodes: ops.length, instances, frames: screens, pages: sizes.length + 1,
      // Every root frame, the component masters included (the All pages board shows them).
      rootFrames: screens + ops.filter((op) => op.parentId === "components").length,
      pageIds: sizes.map((_, p) => `pg-${p}`), fonts: FONTS.length + 1,
    };
  } finally {
    await mcp.close();
  }
}
