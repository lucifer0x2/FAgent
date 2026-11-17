// Fixture resources for the Vello parity run: the native-paint-parity fonts and
// synthetic images, minus Inter (its pinned capture is not in this repository).
import { readFile, writeFile, mkdir } from "node:fs/promises";
import { resolve, join } from "node:path";
import { hash } from "../native-paint-parity/scene.mjs";

const fonts = [
  ["noto", "renderer/fonts/NotoSans.ttf", "FAgent Noto Sans", 400, "normal"],
  ["lato-regular", "fixtures/fonts/Lato-Regular.ttf", "Lato", 400, "normal"],
  ["lato-bold", "fixtures/fonts/Lato-Bold.ttf", "Lato", 700, "normal"],
  ["lato-italic", "fixtures/fonts/Lato-Italic.ttf", "Lato", 400, "italic"],
];

export async function prepareResources(page, root, out) {
  const dir = join(out, "resources");
  await mkdir(dir, { recursive: true });
  const result = { fonts: [], assets: [] };
  for (const [id, source, family, weight, style] of fonts) {
    const bytes = await readFile(resolve(root, source));
    await writeFile(join(dir, `${id}.ttf`), bytes);
    result.fonts.push({ id, family, weight, style, hash: hash(bytes), url: `/resources/${id}.ttf` });
  }
  // Same 63x41 pattern as native-paint-parity, encoded by the pinned Chromium.
  const encoded = await page.evaluate(() => {
    const canvas = document.createElement("canvas"); canvas.width = 63; canvas.height = 41;
    const ctx = canvas.getContext("2d"), data = ctx.createImageData(63, 41);
    for (let y = 0; y < 41; y++) for (let x = 0; x < 63; x++) {
      data.data.set([x < 21 ? 245 : 20, y < 20 ? 35 : 210, x > 40 ? 240 : 30, x < 8 && y < 8 ? 0 : 255], (y * 63 + x) * 4);
    }
    ctx.putImageData(data, 0, 0);
    return ["png", "jpeg", "webp"].map(type => [type, canvas.toDataURL(`image/${type}`, 0.94)]);
  });
  for (const [id, url] of encoded) {
    const bytes = Buffer.from(url.split(",")[1], "base64");
    await writeFile(join(dir, `${id}.bin`), bytes);
    result.assets.push({ id, mediaType: `image/${id}`, hash: hash(bytes), url: `/resources/${id}.bin` });
  }
  return result;
}

/** Drops text nodes whose font is not prepared (Inter) and cases left empty. */
export function withoutMissingFonts(cases, resources) {
  const ids = new Set(resources.fonts.map(f => f.id));
  return cases
    .map(c => ({ ...c, scene: { ...c.scene, nodes: c.scene.nodes.filter(n => n.kind !== "text" || ids.has(n.fontId)) } }))
    .filter(c => c.scene.nodes.length && !c.name.includes("inter"));
}
