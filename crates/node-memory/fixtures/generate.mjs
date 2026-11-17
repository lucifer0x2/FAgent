#!/usr/bin/env node
// Deterministic synthetic project in the stored Snapshot format (schemaVersion 4).
// Usage: node generate.mjs --nodes 200000 --seed 1 --out <file> [--wide 14000]
// --nodes counts expanded nodes (authored + virtual instance children) because the
// scene-input validator caps the expanded total at MAX_NODES; authored is a bit less.
// Nodes are written as they are generated, in 1 MiB chunks, so millions of nodes never
// sit in one string or array. Pages scale with size (4 up to 200k, one per 50k above).
// Plain Node.js >= 20, no dependencies. Output is never committed.
import { closeSync, openSync, writeSync } from "node:fs";

function parseArgs(argv) {
  const args = { nodes: 5000, seed: 1, out: null, wide: 14000 };
  for (let i = 0; i < argv.length; i += 2) {
    const key = argv[i].replace(/^--/, "");
    if (!(key in args) || argv[i + 1] === undefined) {
      throw new Error(`unknown or incomplete argument ${argv[i]}`);
    }
    args[key] = key === "out" ? argv[i + 1] : Number(argv[i + 1]);
  }
  if (!args.out) throw new Error("--out <file> is required");
  return args;
}

// mulberry32: small deterministic PRNG.
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Synchronous chunked writer: each write blocks, so memory stays at one chunk.
function chunkedWriter(path, chunkBytes = 1 << 20) {
  const fd = openSync(path, "w");
  let buffer = "";
  const flush = () => {
    if (buffer) writeSync(fd, buffer);
    buffer = "";
  };
  return {
    write(text) {
      buffer += text;
      if (buffer.length >= chunkBytes) flush();
    },
    close() {
      flush();
      closeSync(fd);
    },
  };
}

const PAGE_NODES = 50_000;
const MASTERS = 8;
const MASTER_CHILDREN = 3;
const FONT = "font_sans";

// The snapshot around its nodes array, in the stored key order.
function envelope(seed) {
  const head = JSON.stringify({
    schemaVersion: 4, projectId: `fixture_${seed}`, slug: `fixture-${seed}`,
    name: `Fixture ${seed}`, revision: 1, nodes: [],
  });
  const tail = JSON.stringify({
    tokens: [], styles: [], assets: [],
    fonts: [{
      id: FONT, family: "Noto Sans", weight: 400, style: "normal", assetId: null,
      bundledId: "noto", contentHash: "fixture", license: "OFL", status: "ready",
    }],
    notes: [], retention: { oldestCommitRevision: 0, idempotencyPolicy: "bounded" },
  });
  return { head: head.slice(0, -2), tail: "]," + tail.slice(1) };
}

function generate({ nodes: target, seed, wide }, out) {
  const random = rng(seed);
  const color = () => "#" + Math.floor(random() * 0xffffff).toString(16).padStart(6, "0");
  let authored = 0;
  let virtual = 0;
  let instances = 0;
  let counter = 0;
  const sortKeys = new Map();
  const total = () => authored + virtual;
  const add = (id, kind, parentId, props) => {
    const sortKey = sortKeys.get(parentId) ?? 0;
    sortKeys.set(parentId, sortKey + 1);
    out.write((authored++ ? "," : "") + JSON.stringify({ id, parentId, sortKey, kind, props }));
    return id;
  };
  const text = (name) => ({
    name, text: `${name} text`, fontId: FONT, fontSize: 14, lineHeight: 20,
  });
  const rect = (name) => ({ name, width: 40 + Math.floor(random() * 200), height: 24, fill: color() });
  const frame = (name, width, height) => ({
    name, width, height, fill: color(), layout: { mode: "column", gap: 8 },
  });

  const pageCount = Math.max(4, Math.ceil(target / PAGE_NODES));
  add("page_library", "page", null, { name: "Library" });
  const pages = [];
  for (let p = 1; p <= pageCount; p++) pages.push(add(`page_${p}`, "page", null, { name: `Page ${p}` }));
  for (let m = 0; m < MASTERS; m++) {
    const id = add(`master_${m}`, "frame", "page_library", {
      ...frame(`Master ${m}`, 240, 96), component: { name: `Component ${m}` },
    });
    add(`${id}_title`, "text", id, text(`Title ${m}`));
    add(`${id}_icon`, "rectangle", id, rect(`Icon ${m}`));
    add(`${id}_body`, "text", id, text(`Body ${m}`));
  }

  const wideCount = Math.max(0, Math.min(wide, Math.floor(target / 4)));
  add("frame_wide", "frame", pages[0], { name: "Wide", width: 1440, height: 900 });
  for (let i = 0; i < wideCount && total() < target; i++) {
    const id = `wide_${i}`;
    if (i % 2 === 0) add(id, "rectangle", "frame_wide", rect(`Cell ${i}`));
    else add(id, "text", "frame_wide", text(`Cell ${i}`));
  }

  let roots = 0;
  for (let r = 0; total() < target; r++, roots++) {
    const root = add(`root_${r}`, "frame", pages[r % pageCount], frame(`Screen ${r}`, 1440, 1024));
    const open = [{ id: root, depth: 0 }];
    const size = 50 + Math.floor(random() * 350);
    for (let j = 0; j < size && total() < target; j++) {
      const parent = open[Math.floor(random() * open.length)];
      const id = `n${(counter++).toString(36)}`;
      const pick = random();
      if (pick < 0.15 && parent.depth < 4) {
        add(id, "frame", parent.id, frame(`Section ${id}`, 1200, 240));
        open.push({ id, depth: parent.depth + 1 });
      } else if (pick < 0.17 && total() + 1 + MASTER_CHILDREN <= target) {
        const m = Math.floor(random() * MASTERS);
        add(id, "instance", parent.id, {
          componentId: `master_${m}`, name: `Card ${id}`,
          overrides: { [`master_${m}_title`]: { text: `Title ${id}` } },
        });
        virtual += MASTER_CHILDREN;
        instances += 1;
      } else if (pick < 0.65) {
        add(id, "text", parent.id, text(`Label ${id}`));
      } else {
        add(id, "rectangle", parent.id, rect(`Box ${id}`));
      }
    }
  }
  return {
    authored, expanded: total(), instances, wideChildren: wideCount, pages: pageCount, roots,
  };
}

const args = parseArgs(process.argv.slice(2));
const out = chunkedWriter(args.out);
const { head, tail } = envelope(args.seed);
out.write(head);
const stats = generate(args, out);
out.write(tail);
out.close();
process.stdout.write(JSON.stringify(stats) + "\n");
