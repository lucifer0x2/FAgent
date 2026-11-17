// The viewer defaults to the Vello engine; checks of the Canvas2D painter, its scene paths or
// its canvases pin Canvas2D explicitly (same preference a user can save).
export const pinCanvas2d = (target) =>
  target.addInitScript(() => {
    try {
      localStorage.setItem("fagent.renderer", "canvas2d");
    } catch {}
  });
