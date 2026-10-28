import { api } from "./api";
import type { Doc } from "./model";

// Closing a dialog cannot cancel an IPC render that is already running. Keep
// canvas and draft requests serialized even while their components change.
let queue: Promise<void> = Promise.resolve();
export function renderPreview(doc: Doc) {
  const result = queue.then(() => api.render(doc));
  queue = result.then(
    () => {},
    () => {},
  );
  return result;
}
