import { create } from "zustand";
import { api } from "./api";
import {
  active,
  validate,
  type Doc,
  type Layer,
  type Kind,
  layer,
} from "./model";
type Revision = Pick<Doc, "scenes" | "activeSceneId" | "bindings">;
const revision = (doc: Doc): Revision =>
  structuredClone({
    scenes: doc.scenes,
    activeSceneId: doc.activeSceneId,
    bindings: doc.bindings,
  });
interface Store {
  doc: Doc | null;
  selected: string[];
  past: Revision[];
  future: Revision[];
  serial: number;
  saved: number;
  error: string;
  busy: boolean;
  edit: (fn: (doc: Doc) => void, history?: boolean) => void;
  select: (ids: string[]) => void;
  patch: (id: string, patch: Partial<Layer>, history?: boolean) => void;
  add: (kind: Kind) => void;
  remove: () => void;
  undo: () => void;
  redo: () => void;
  accept: (doc: Doc) => void;
  begin: () => void;
}
let timer: ReturnType<typeof setTimeout> | undefined;
let saving: Promise<void> | null = null;
function schedule() {
  clearTimeout(timer);
  timer = setTimeout(() => void flush().catch(() => {}), 350);
}
export const useStore = create<Store>((set, get) => ({
  doc: null,
  selected: [],
  past: [],
  future: [],
  serial: 0,
  saved: 0,
  error: "",
  busy: false,
  edit: (fn, history = true) => {
    const s = get();
    if (!s.doc || s.busy) return;
    const doc = structuredClone(s.doc);
    try {
      fn(doc);
      validate(doc);
    } catch (e) {
      set({ error: String(e) });
      return;
    }
    set({
      doc,
      serial: s.serial + 1,
      error: "",
      ...(history
        ? { past: [...s.past, revision(s.doc)].slice(-100), future: [] }
        : {}),
    });
    schedule();
  },
  select: (selected) => set({ selected }),
  patch: (id, patch, history = true) =>
    get().edit((doc) => {
      const l = active(doc).layers.find((l) => l.id === id);
      if (l) Object.assign(l, patch);
    }, history),
  add: (kind) => {
    const l = layer(kind);
    get().edit((doc) => active(doc).layers.push(l));
    if (get().doc && active(get().doc!).layers.some((x) => x.id === l.id))
      set({ selected: [l.id] });
  },
  remove: () => {
    const ids = get().selected;
    get().edit((doc) => {
      active(doc).layers = active(doc).layers.filter(
        (l) => !ids.includes(l.id) || l.locked,
      );
    });
    set({ selected: [] });
  },
  undo: () => {
    const s = get(),
      r = s.past.at(-1);
    if (!s.doc || !r || s.busy) return;
    set({
      doc: { ...s.doc, ...structuredClone(r) },
      past: s.past.slice(0, -1),
      future: [revision(s.doc), ...s.future],
      selected: [],
      serial: s.serial + 1,
    });
    schedule();
  },
  redo: () => {
    const s = get(),
      r = s.future[0];
    if (!s.doc || !r || s.busy) return;
    set({
      doc: { ...s.doc, ...structuredClone(r) },
      past: [...s.past, revision(s.doc)],
      future: s.future.slice(1),
      selected: [],
      serial: s.serial + 1,
    });
    schedule();
  },
  accept: (doc) => {
    const s = get();
    set({
      doc,
      selected: [],
      past: s.doc ? [...s.past, revision(s.doc)].slice(-100) : [],
      future: [],
      serial: s.serial + 1,
      saved: s.serial + 1,
      error: "",
    });
  },
  begin: () => {
    const s = get();
    if (s.doc)
      set({ past: [...s.past, revision(s.doc)].slice(-100), future: [] });
  },
}));
export async function flush(): Promise<void> {
  clearTimeout(timer);
  if (saving) {
    await saving;
    if (useStore.getState().saved !== useStore.getState().serial)
      return flush();
    return;
  }
  const work = async () => {
    while (true) {
      const { doc, serial, saved } = useStore.getState();
      if (!doc || serial === saved) return;
      try {
        await api.save(doc);
        useStore.setState({ saved: serial, error: "" });
      } catch (e) {
        useStore.setState({ error: `保存失败：${String(e)}` });
        throw e;
      }
    }
  };
  saving = work();
  try {
    await saving;
  } finally {
    saving = null;
  }
}
export async function operation<T>(job: () => Promise<T>): Promise<T> {
  await flush();
  useStore.setState({ busy: true });
  try {
    return await job();
  } finally {
    useStore.setState({ busy: false });
  }
}
