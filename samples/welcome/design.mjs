// A small authored sample. This module has no network or filesystem side effects.
export const FRAME = "welcome-dashboard";
export const VIEWPORT = { width: 1040, height: 720 };

export function welcomeOperations() {
  const operations = [];
  const token = (id, name, value) => operations.push({
    op: "upsert_token", token: { id, name, type: "color", value },
  });
  const style = (id, name, tokenId) => operations.push({
    op: "upsert_style", style: { id, name, kind: "fill", value: { tokenId } },
  });
  const node = (id, kind, parentId, props) => operations.push({
    op: "create_node", id, kind, parentId, props: { name: id, ...props },
  });
  const surface = { styleId: "surface-fill" };
  const brand = { styleId: "brand-fill" };
  const ink = { tokenId: "ink" };
  const muted = { tokenId: "muted" };
  const box = (id, parent, x, y, width, height, fill, radius = 0) =>
    node(id, "rectangle", parent, { x, y, width, height, fill, radius });
  const group = (id, parent, x, y, width, height, fill, radius = 16) =>
    node(id, "group", parent, {
      x, y, width, height, ...(fill ? { fill } : {}), radius,
      layout: { mode: "free" },
    });
  const text = (id, parent, copy, x, y, width, size = 14, fill = ink) =>
    node(id, "text", parent, {
      text: copy, x, y, width, height: "hug", fontId: "font_bundled",
      fontSize: size, lineHeight: Math.ceil(size * 1.4), fill,
    });

  token("brand", "Brand / Indigo", "#4F46E5");
  token("surface", "Surface / White", "#FFFFFF");
  token("canvas", "Canvas / Mist", "#F5F6FA");
  token("ink", "Text / Primary", "#1B2440");
  token("muted", "Text / Secondary", "#667085");
  style("brand-fill", "Brand surface", "brand");
  style("surface-fill", "Card surface", "surface");
  node("welcome-page", "page", null, { name: "First sample" });
  node(FRAME, "frame", "welcome-page", {
    name: "A good day", ...VIEWPORT, fill: { tokenId: "canvas" },
    layout: { mode: "free" }, clip: true,
  });

  group("sidebar", FRAME, 0, 0, 176, 720, surface, 0);
  box("logo", "sidebar", 24, 32, 32, 32, brand, 9);
  text("logo-letter", "sidebar", "o", 33, 30, 20, 25, "#FFFFFF");
  text("brand-name", "sidebar", "Orbit", 67, 33, 96, 21);
  text("workspace-label", "sidebar", "YOUR WORKSPACE", 24, 104, 145, 10, muted);
  box("active-nav", "sidebar", 16, 136, 144, 44, "#EEEDFF", 10);
  text("overview-nav", "sidebar", "Overview", 32, 146, 118, 14, brand);
  text("projects-nav", "sidebar", "Projects", 32, 200, 118, 14, muted);
  text("calendar-nav", "sidebar", "Calendar", 32, 246, 125, 14, muted);
  box("sidebar-divider", "sidebar", 24, 625, 128, 1, "#E9ECF2");
  text("profile-name", "sidebar", "Creative Studio", 24, 646, 144, 12);
  text("profile-plan", "sidebar", "Slower, but better.", 24, 669, 145, 10, muted);

  text("date", FRAME, "WEDNESDAY, OCTOBER 28", 208, 28, 420, 11, muted);
  text("greeting", FRAME, "A good day.", 208, 50, 600, 30);
  text("intro", FRAME, "Focus on what matters, one task at a time.", 208, 99, 570, 13, muted);
  box("new-task-button", FRAME, 852, 47, 140, 42, brand, 10);
  text("new-task-label", FRAME, "+  New task", 868, 58, 120, 13, "#FFFFFF");

  const stats = [
    ["completed", "Completed", "24", "+6 vs last week", "#178061"],
    ["active", "In progress", "08", "3 tasks need you today", "#667085"],
    ["focus", "Focus time", "18.5 hours", "Steady every day", "#667085"],
  ];
  stats.forEach(([id, label, value, note, color], index) => {
    group(id, FRAME, 208 + index * 268, 148, 248, 112, surface);
    text(`${id}-label`, id, label, 20, 16, 216, 12, muted);
    text(`${id}-value`, id, value, 20, 38, 216, 28);
    text(`${id}-note`, id, note, 20, 83, 216, 11, color);
  });

  group("weekly-chart", FRAME, 208, 280, 498, 238, surface);
  text("chart-title", "weekly-chart", "Work rhythm", 22, 18, 300, 17);
  text("chart-subtitle", "weekly-chart", "Tasks completed in the last 7 days", 22, 47, 330, 11, muted);
  box("chart-legend-dot", "weekly-chart", 393, 26, 8, 8, brand, 3);
  text("chart-legend", "weekly-chart", "This week", 409, 21, 82, 10, muted);
  [88, 138, 188].forEach((y, index) =>
    box(`chart-grid-${index}`, "weekly-chart", 24, y, 450, 1, "#EEF0F5"));
  [3, 5, 4, 7, 6, 9, 8].forEach((value, index) => {
    const x = 39 + index * 62;
    const height = value * 11;
    box(`day-${index}-bar`, "weekly-chart", x, 188 - height, 28, height,
      index === 5 ? brand : "#C7C4FC", 5);
    text(`day-${index}-label`, "weekly-chart", ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][index],
      x + 3, 204, 40, 10, muted);
  });

  group("weekly-goal", FRAME, 726, 280, 266, 238, "#EEEDFF");
  text("goal-label", "weekly-goal", "WEEKLY GOAL", 22, 23, 230, 10, "#514A9D");
  text("goal-value", "weekly-goal", "80%", 22, 49, 230, 38, brand);
  text("goal-caption", "weekly-goal", "You're keeping a great pace.", 22, 113, 230, 13);
  box("goal-track", "weekly-goal", 22, 158, 222, 8, "#DCD9FB", 4);
  box("goal-progress", "weekly-goal", 22, 158, 178, 8, brand, 4);
  text("goal-details", "weekly-goal", "24 / 30 tasks completed", 22, 184, 230, 11, "#514A9D");

  group("today-tasks", FRAME, 208, 538, 784, 150, surface);
  text("tasks-title", "today-tasks", "Today's priorities", 22, 17, 380, 17);
  text("tasks-count", "today-tasks", "2 tasks to focus on", 604, 22, 170, 11, muted);
  box("task-divider", "today-tasks", 22, 53, 740, 1, "#EEF0F5");
  [
    ["review", "Finish the home page design", "In progress", "#EEEDFF", "#5145BC"],
    ["feedback", "Read feedback from the design team", "Up next", "#F2F4F7", "#667085"],
  ].forEach(([id, copy, status, background, foreground], index) => {
    const y = 69 + index * 41;
    box(`${id}-checkbox`, "today-tasks", 22, y, 17, 17, "#E8EBF2", 5);
    text(`${id}-text`, "today-tasks", copy, 52, y - 2, 510, 13);
    box(`${id}-status`, "today-tasks", 662, y - 4, 100, 26, background, 13);
    text(`${id}-status-label`, "today-tasks", status, 676, y + 1, 80, 11, foreground);
  });
  return operations;
}
