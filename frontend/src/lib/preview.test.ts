import { it, expect, vi } from "vitest";
vi.mock("./api", () => ({ api: { render: vi.fn(), thumbnail: vi.fn() } }));
import { api } from "./api";
import { renderPreview, renderThumbnail } from "./preview";
import type { Doc } from "./model";
it("serializes canvas/draft IPC requests and releases the queue after failures", async () => {
  let release!: () => void;
  const frame = { width: 1, height: 1, rgba: "", sequence: 1 };
  vi.mocked(api.render)
    .mockImplementationOnce(
      () =>
        new Promise((_, reject) => {
          release = () => reject(Error("failed"));
        }),
    )
    .mockResolvedValue(frame);
  const first = renderPreview({} as Doc);
  const caught = first.catch((e) => e.message);
  const second = renderPreview({} as Doc);
  await Promise.resolve();
  expect(api.render).toHaveBeenCalledTimes(1);
  release();
  expect(await caught).toBe("failed");
  expect(await second).toEqual(frame);
  expect(api.render).toHaveBeenCalledTimes(2);
});

it("prioritizes interactive work and removes obsolete queued thumbnails", async () => {
  const order: string[] = [];
  const frame = { width: 1, height: 1, rgba: "", sequence: 1 };
  let release!: (value: typeof frame) => void;
  vi.mocked(api.render)
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          order.push("active");
          release = resolve;
        }),
    )
    .mockImplementation(async () => {
      order.push("editor");
      return frame;
    });
  vi.mocked(api.thumbnail).mockImplementation(async (_, id) => {
    order.push(id);
    return frame;
  });
  const current = renderPreview({} as Doc);
  await Promise.resolve();
  const obsolete = new AbortController();
  const discarded = renderThumbnail(
    {} as Doc,
    "obsolete",
    obsolete.signal,
  ).catch((e) => e.name);
  const background = renderThumbnail(
    {} as Doc,
    "thumbnail",
    new AbortController().signal,
  );
  const editor = renderPreview({} as Doc);
  obsolete.abort();
  expect(await discarded).toBe("AbortError");
  expect(order).toEqual(["active"]);
  release(frame);
  await Promise.all([current, editor, background]);
  expect(order).toEqual(["active", "editor", "thumbnail"]);
});

it("drops a canceled in-flight result without overlapping the next request", async () => {
  const frame = { width: 1, height: 1, rgba: "", sequence: 1 };
  const controller = new AbortController();
  let release!: (value: typeof frame) => void;
  vi.mocked(api.thumbnail).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  const canceled = renderThumbnail({} as Doc, "old", controller.signal).catch(
    (e) => e.name,
  );
  await Promise.resolve();
  controller.abort();
  const count = vi.mocked(api.render).mock.calls.length;
  const next = renderPreview({} as Doc);
  await Promise.resolve();
  expect(vi.mocked(api.render).mock.calls.length).toBe(count);
  release(frame);
  expect(await canceled).toBe("AbortError");
  await next;
  expect(vi.mocked(api.render).mock.calls.length).toBe(count + 1);
});
