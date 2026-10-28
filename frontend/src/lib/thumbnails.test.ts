import { expect, it } from "vitest";
import { blankScene, layer, type Doc } from "./model";
import {
  dynamicScene,
  pruneThumbnails,
  thumbnailCache,
  thumbnailKey,
} from "./thumbnails";

it("invalidates only affected scenes and referenced media/preferences", () => {
  const scene = blankScene();
  const other = blankScene();
  const doc = {
    scenes: [scene, other],
    assets: [],
    deepseek: { currency: "CNY" },
    activeSceneId: scene.id,
  } as unknown as Doc;
  const original = thumbnailKey(doc, scene);
  doc.activeSceneId = other.id;
  other.layers.push(layer("rect"));
  expect(thumbnailKey(doc, scene)).toBe(original);
  scene.layers.push(layer("text"));
  expect(thumbnailKey(doc, scene)).not.toBe(original);
  const text = thumbnailKey(doc, scene);
  doc.deepseek.currency = "USD";
  expect(thumbnailKey(doc, scene)).not.toBe(text);
  const prefs = thumbnailKey(doc, scene);
  doc.assets.push({
    id: "media",
    name: "test",
    file: "test.png",
    kind: "image",
    width: 20,
    height: 20,
  });
  expect(thumbnailKey(doc, scene)).toBe(prefs);
  scene.layers[0].assetId = "media";
  const referenced = thumbnailKey(doc, scene);
  doc.assets[0].file = "updated.png";
  expect(thumbnailKey(doc, scene)).not.toBe(referenced);
  thumbnailCache.set(scene.id, {
    key: referenced,
    url: "data:image/png;base64,",
    time: 0,
  });
  thumbnailCache.set(other.id, { key: "", url: "", time: 0 });
  doc.scenes = [scene];
  pruneThumbnails(doc);
  expect([...thumbnailCache.keys()]).toEqual([scene.id]);
  thumbnailCache.clear();
});

it("refreshes visible runtime fields and skips hidden or static content", () => {
  const scene = blankScene();
  const text = layer("text");
  scene.layers.push(text);
  expect(dynamicScene(scene)).toBe(false);
  text.richText = {
    version: 1,
    runs: [
      {
        type: "deepseek",
        metric: "balance",
        decimals: 4,
        showCurrency: true,
        style: {},
      },
    ],
  };
  expect(dynamicScene(scene)).toBe(true);
  text.visible = false;
  expect(dynamicScene(scene)).toBe(false);
  for (const kind of ["sensor", "clock", "video", "bar", "deepseek"] as const) {
    scene.layers = [layer(kind)];
    expect(dynamicScene(scene)).toBe(true);
  }
});
