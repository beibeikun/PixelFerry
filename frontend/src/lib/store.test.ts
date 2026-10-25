import { beforeEach, afterEach, describe, it, expect, vi } from "vitest";
vi.mock("./api", () => ({ api: { save: vi.fn(async () => {}) } }));
import { api } from "./api";
import { useStore, flush, operation } from "./store";
import { blankScene, layer, type Doc } from "./model";
function fixture(): Doc {
  const scene = blankScene();
  scene.layers.push(layer("text"));
  return {
    version: 2,
    scenes: [scene],
    activeSceneId: scene.id,
    assets: [],
    settings: {
      brightness: 80,
      direction: 0,
      power: true,
      autoStart: false,
      quality: 70,
      renderFps: 30,
    },
    deepseek: { currency: "CNY", holidays: [], calendarYears: [2026] },
    bindings: [],
  };
}
beforeEach(() => {
  vi.useFakeTimers();
  vi.clearAllMocks();
  useStore.setState({
    doc: fixture(),
    selected: [],
    past: [],
    future: [],
    serial: 0,
    saved: 0,
    error: "",
    busy: false,
  });
});
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});
describe("workspace transactions", () => {
  it("undoes an entire drag transaction, then redoes it", () => {
    const s = useStore.getState(),
      before = s.doc!.scenes[0].layers[0].x;
    s.begin();
    s.edit((d) => {
      d.scenes[0].layers[0].x += 10;
    }, false);
    s.edit((d) => {
      d.scenes[0].layers[0].x += 20;
    }, false);
    expect(useStore.getState().past).toHaveLength(1);
    s.undo();
    expect(useStore.getState().doc!.scenes[0].layers[0].x).toBe(before);
    s.redo();
    expect(useStore.getState().doc!.scenes[0].layers[0].x).toBe(before + 30);
  });
  it("rejects two visible dynamic media layers", () => {
    const s = useStore.getState();
    s.add("video");
    s.add("video");
    expect(
      useStore
        .getState()
        .doc!.scenes[0].layers.filter((l) => l.type === "video"),
    ).toHaveLength(1);
    expect(useStore.getState().error).toContain("动态");
  });
  it("keeps imported assets available when undoing scene import", () => {
    const s = useStore.getState(),
      d = structuredClone(s.doc!);
    d.scenes.push(blankScene());
    d.assets.push({
      id: "asset",
      name: "image",
      file: "asset.png",
      kind: "image",
      width: 16,
      height: 16,
    });
    s.accept(d);
    s.undo();
    expect(useStore.getState().doc!.scenes).toHaveLength(1);
    expect(useStore.getState().doc!.assets).toHaveLength(1);
    s.redo();
    expect(useStore.getState().doc!.scenes).toHaveLength(2);
  });
  it("waits for the latest revision before completing a save barrier", async () => {
    let release!: () => void;
    vi.mocked(api.save).mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          release = resolve;
        }),
    );
    useStore.getState().edit((d) => {
      d.scenes[0].name = "first";
    });
    const saved = flush();
    useStore.getState().edit((d) => {
      d.scenes[0].name = "latest";
    });
    release();
    await saved;
    expect(api.save).toHaveBeenCalledTimes(2);
    expect(vi.mocked(api.save).mock.calls[1][0].scenes[0].name).toBe("latest");
    expect(useStore.getState().saved).toBe(useStore.getState().serial);
  });
  it("does not edit the workspace while a file exchange is busy", async () => {
    let finish!: () => void;
    const pending = operation(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    await Promise.resolve();
    await Promise.resolve();
    expect(useStore.getState().busy).toBe(true);
    useStore.getState().edit((d) => {
      d.scenes[0].name = "should not be committed";
    });
    expect(useStore.getState().doc!.scenes[0].name).toBe("新的航线");
    finish();
    await pending;
    expect(useStore.getState().busy).toBe(false);
  });
  it("never deletes a locked selected layer", () => {
    const s = useStore.getState(),
      l = s.doc!.scenes[0].layers[0];
    s.patch(l.id, { locked: true });
    s.select([l.id]);
    s.remove();
    expect(useStore.getState().doc!.scenes[0].layers).toHaveLength(1);
  });
});
