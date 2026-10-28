import { it, expect, vi } from "vitest";
vi.mock("./api", () => ({ api: { render: vi.fn() } }));
import { api } from "./api";
import { renderPreview } from "./preview";
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
