// Feature groups of the parity fixtures (the P0 table in docs/VELLO-GPU-RENDERER-RESEARCH.md).
export const GROUPS = [
  [/^(solid|scale)/, "solid + fractional scale"], [/^(linear|gradient|radial)/, "gradients"],
  [/^per-corner/, "per-corner radius"], [/^stroke-inside/, "stroke inside"], [/^stroke-center/, "stroke center"],
  [/^stroke-outside/, "stroke outside"], [/^(nested|group)/, "opacity + clip"], [/^shadow-inset/, "shadow inset"],
  [/^shadow/, "shadow outer"], [/^image/, "images"], [/^text/, "text"], [/^full-nontext/, "combined non-text"],
  [/^full-text/, "combined text"],
];
export const groupOf = name => GROUPS.find(([re]) => re.test(name))?.[1] ?? "other";

// One row per group: `metrics(row)` returns {verdict, straight, white} (fractions > 8) or null.
export function groupTable(rows, metrics) {
  const groups = new Map();
  for (const row of rows) {
    const m = metrics(row);
    if (!m) continue;
    const g = groupOf(row.name);
    if (!groups.has(g)) groups.set(g, []);
    groups.get(g).push(m);
  }
  return [...groups].map(([group, ms]) => ({
    group, cases: ms.length,
    pass: ms.every(m => m.verdict === "REVIEW_TEXT") ? "review" : `${ms.filter(m => m.verdict === "PASS").length}/${ms.length}`,
    maxStraight: Math.max(...ms.map(m => m.straight)), maxWhite: Math.max(...ms.map(m => m.white)),
  }));
}

export const markdown = (title, table) => [`\n${title}`, "| group | cases | pass | max % >8 straight | max % >8 white |", "| --- | --- | --- | --- | --- |",
  ...table.map(r => `| ${r.group} | ${r.cases} | ${r.pass} | ${(r.maxStraight * 100).toFixed(2)} | ${(r.maxWhite * 100).toFixed(2)} |`)].join("\n");
