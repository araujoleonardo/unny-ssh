/** One keyboard/outside-click lifecycle for project pickers and context menus. */
export class Popup {
  private trigger?: HTMLElement;
  private open = false;
  constructor(readonly panel: HTMLElement) {
    panel.hidden = true;
    panel.classList.add("ide-popup");
    panel.addEventListener("keydown", this.key);
  }
  private outside = (event: PointerEvent) => {
    const target = event.target as Node;
    if (!this.panel.contains(target) && !this.trigger?.contains(target))
      this.close(false);
  };
  private key = (event: KeyboardEvent) => {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      this.close();
      return;
    }
    if (event.key === "Tab") {
      this.close(false);
      return;
    }
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    if (
      event.target instanceof HTMLInputElement &&
      ["Home", "End"].includes(event.key)
    )
      return;
    const items = [
      ...this.panel.querySelectorAll<HTMLButtonElement>(
        "button:not(:disabled)",
      ),
    ].filter((b) => !b.hidden);
    if (!items.length) return;
    event.preventDefault();
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    const n =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? items.length - 1
          : event.key === "ArrowDown"
            ? (i + 1) % items.length
            : (i - 1 + items.length) % items.length;
    items[n].focus();
  };
  show(trigger: HTMLElement, x?: number, y?: number) {
    this.trigger = trigger;
    this.open = true;
    this.panel.hidden = false;
    if (trigger.hasAttribute("aria-controls"))
      trigger.setAttribute("aria-expanded", "true");
    const box = trigger.getBoundingClientRect();
    const width = this.panel.offsetWidth || 320,
      height = this.panel.offsetHeight || 300;
    this.panel.style.left = `${Math.max(6, Math.min(x ?? box.left, window.innerWidth - width - 6))}px`;
    this.panel.style.top = `${Math.max(6, Math.min(y ?? box.bottom + 6, window.innerHeight - height - 6))}px`;
    document.addEventListener("pointerdown", this.outside, true);
    (
      this.panel.querySelector("input") ?? this.panel.querySelector("button")
    )?.focus();
  }
  toggle(trigger: HTMLElement) {
    if (this.open) this.close();
    else this.show(trigger);
  }
  close(focus = true) {
    if (!this.open) return;
    this.open = false;
    this.panel.hidden = true;
    if (this.trigger?.hasAttribute("aria-controls"))
      this.trigger.setAttribute("aria-expanded", "false");
    document.removeEventListener("pointerdown", this.outside, true);
    if (focus && this.trigger?.isConnected) this.trigger.focus();
  }
  dispose() {
    this.close(false);
    this.panel.removeEventListener("keydown", this.key);
    this.panel.remove();
  }
}
