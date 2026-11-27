/**
 * Yields to the worker's event loop between two frame computations, so page messages that
 * arrived meanwhile (camera, layout, a newer sync) run before the next frame.
 *
 * A `setTimeout(0)` chain is clamped to 4 ms once timers nest five deep (each yield is
 * scheduled from the previous timer's task), which left the worker idle ~4 ms per frame:
 * about a second over a 229-frame board. A MessageChannel message is a task of the same
 * posted-message source as the page's messages, queued after the ones already received,
 * with no clamp. Without MessageChannel the timer is kept.
 */
export function messageYield(): () => Promise<void> {
  if (typeof MessageChannel === "undefined")
    return () => new Promise((resolve) => setTimeout(resolve, 0));
  const channel = new MessageChannel();
  const waiting: (() => void)[] = [];
  channel.port1.onmessage = () => waiting.shift()?.();
  return () =>
    new Promise((resolve) => {
      waiting.push(resolve);
      channel.port2.postMessage(null);
    });
}
