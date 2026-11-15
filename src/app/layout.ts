export interface Layout {
  sidebar: number;
  terminal: number;
  sidebarHidden: boolean;
  terminalHidden: boolean;
}
export const defaults: Layout = {
  sidebar: 280,
  terminal: 230,
  sidebarHidden: false,
  terminalHidden: false,
};
const key = "unny.layout.v1";
export function readLayout(): Layout {
  try {
    const data = JSON.parse(localStorage.getItem(key) ?? "null");
    if (!data || typeof data !== "object") return { ...defaults };
    return {
      sidebar:
        typeof data.sidebar === "number" && Number.isFinite(data.sidebar)
          ? Math.min(420, Math.max(190, data.sidebar))
          : defaults.sidebar,
      terminal:
        typeof data.terminal === "number" && Number.isFinite(data.terminal)
          ? Math.min(460, Math.max(140, data.terminal))
          : defaults.terminal,
      sidebarHidden: data.sidebarHidden === true,
      terminalHidden: data.terminalHidden === true,
    };
  } catch {
    return { ...defaults };
  }
}
export function saveLayout(layout: Layout) {
  try {
    localStorage.setItem(key, JSON.stringify(layout));
  } catch {
    /* Layout remains usable without storage. */
  }
}
export function bindDivider(
  element: HTMLElement,
  axis: "x" | "y",
  get: () => number,
  set: (n: number) => void,
) {
  const update = (n: number) => {
    set(n);
    element.setAttribute("aria-valuenow", String(get()));
  };
  element.addEventListener("keydown", (event) => {
    const keys =
      axis === "x" ? ["ArrowLeft", "ArrowRight"] : ["ArrowDown", "ArrowUp"];
    if (keys.includes(event.key)) {
      event.preventDefault();
      update(get() + (event.key === keys[0] ? -20 : 20));
    }
    if (event.key === "Home") {
      event.preventDefault();
      update(axis === "x" ? 190 : 140);
    }
    if (event.key === "End") {
      event.preventDefault();
      update(axis === "x" ? 420 : 460);
    }
  });
  element.addEventListener("pointerdown", (event) => {
    const start = axis === "x" ? event.clientX : event.clientY;
    const initial = get();
    element.setPointerCapture(event.pointerId);
    const move = (e: PointerEvent) =>
      update(initial + (axis === "x" ? e.clientX - start : start - e.clientY));
    const stop = () => {
      element.removeEventListener("pointermove", move);
      element.removeEventListener("pointerup", stop);
      element.removeEventListener("pointercancel", stop);
    };
    element.addEventListener("pointermove", move);
    element.addEventListener("pointerup", stop);
    element.addEventListener("pointercancel", stop);
  });
}
