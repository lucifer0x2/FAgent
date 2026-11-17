// Page side of the engine parity capture: one Worker + one transferred canvas per backend.
let sequence = 0;
window.engineCapture = async function capture({ backend, resources, base, jobs, analytic }) {
  const canvas = document.createElement("canvas");
  document.body.append(canvas);
  const offscreen = canvas.transferControlToOffscreen();
  const worker = new Worker(new URL("./worker.mjs", import.meta.url), { type: "module" });
  const replies = new Map();
  worker.onmessage = ({ data }) => replies.get(data.id)?.(data);
  const call = (message, transfer = []) =>
    new Promise((resolve) => {
      const id = ++sequence;
      replies.set(id, resolve);
      worker.postMessage({ ...message, id }, transfer);
    });
  try {
    const init = await call({ type: "init", canvas: offscreen, backend, resources, base, analytic }, [offscreen]);
    if (init.error) return { backend: null, error: init.error, shots: {} };
    const shots = {};
    for (const job of jobs) shots[job.key] = await call({ type: "shot", ...job });
    return { backend: init.backend, failed: init.failed, shots };
  } finally {
    worker.terminate();
    canvas.remove();
  }
};
window.engineCaptureReady = true;
