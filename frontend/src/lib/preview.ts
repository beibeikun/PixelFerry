import { api } from "./api";
import type { Doc, Preview } from "./model";

type Job = {
  run: () => Promise<Preview>;
  resolve: (frame: Preview) => void;
  reject: (error: unknown) => void;
  signal?: AbortSignal;
  cleanup?: () => void;
};
const interactive: Job[] = [];
const thumbnails: Job[] = [];
let running = false;
const aborted = () => new DOMException("预览任务已取消", "AbortError");

// IPC cannot cancel an active render. Keep it serialized across unmounts and
// dialogs, but let queued interactive work run before background thumbnails.
async function pump() {
  if (running) return;
  running = true;
  try {
    let job: Job | undefined;
    while ((job = interactive.shift() ?? thumbnails.shift())) {
      job.cleanup?.();
      if (job.signal?.aborted) {
        job.reject(aborted());
        continue;
      }
      try {
        const frame = await job.run();
        if (job.signal?.aborted) job.reject(aborted());
        else job.resolve(frame);
      } catch (error) {
        job.reject(error);
      }
    }
  } finally {
    running = false;
  }
}
function enqueue(run: Job["run"], background = false, signal?: AbortSignal) {
  return new Promise<Preview>((resolve, reject) => {
    if (signal?.aborted) {
      reject(aborted());
      return;
    }
    const queue = background ? thumbnails : interactive;
    const job: Job = { run, resolve, reject, signal };
    const cancel = () => {
      const index = queue.indexOf(job);
      if (index >= 0) {
        queue.splice(index, 1);
        job.cleanup?.();
        reject(aborted());
      }
    };
    signal?.addEventListener("abort", cancel, { once: true });
    job.cleanup = () => signal?.removeEventListener("abort", cancel);
    queue.push(job);
    queueMicrotask(() => void pump());
  });
}
export function renderPreview(doc: Doc) {
  return enqueue(() => api.render(doc));
}
export function renderThumbnail(
  doc: Doc,
  sceneId: string,
  signal: AbortSignal,
) {
  return enqueue(() => api.thumbnail(doc, sceneId), true, signal);
}
