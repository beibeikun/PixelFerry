import { invoke } from "@tauri-apps/api/core";
import type { Doc, Snapshot, Preview, Asset } from "./model";
export const native = () => "__TAURI_INTERNALS__" in window;
export const api = {
  load: () => invoke<Doc>("load_workspace"),
  save: (doc: Doc) => invoke<void>("save_workspace", { doc }),
  snapshot: (device?: string, screen?: string) =>
    invoke<Snapshot>("snapshot", {
      device: device ?? null,
      screen: screen ?? null,
    }),
  render: (doc: Doc) => invoke<Preview>("render_editor", { doc }),
  media: () => invoke<Asset | null>("import_media"),
  export: (doc: Doc, all: boolean) =>
    invoke<boolean>("export_scenes", { doc, all }),
  import: (doc: Doc) => invoke<Doc | null>("import_scenes", { doc }),
  migrate: (doc: Doc) =>
    invoke<{ doc: Doc; notices: string[] } | null>("migrate_legacy", { doc }),
  reconnect: (device: string) => invoke<void>("reconnect", { device }),
  key: (key: string | null) => invoke<void>("set_key", { key }),
  refresh: () => invoke<void>("refresh_account"),
  quit: () => invoke<void>("quit_app"),
  hide: () => invoke<void>("hide_window"),
  info: () => invoke<{ version: string; simulate: boolean }>("app_info"),
};
