interface SelectView {
  button: HTMLButtonElement;
  listbox: HTMLDivElement;
  sync: () => void;
  close: (restoreFocus?: boolean) => void;
}

const views = new WeakMap<HTMLSelectElement, SelectView>();
let openView: SelectView | undefined;

export function enhanceSelects(root: ParentNode = document): void {
  root.querySelectorAll<HTMLSelectElement>("select").forEach(enhanceSelect);
}

export function enhanceSelect(select: HTMLSelectElement): void {
  if (views.has(select)) return;
  const wrapper = document.createElement("div");
  wrapper.className = "custom-select";
  select.before(wrapper);
  wrapper.append(select);
  select.classList.add("native-select-proxy");
  select.tabIndex = -1;

  const button = document.createElement("button");
  button.type = "button";
  button.className = "custom-select-trigger";
  button.setAttribute("role", "combobox");
  button.setAttribute("aria-haspopup", "listbox");
  button.setAttribute("aria-expanded", "false");
  const valueLabel = document.createElement("span");
  valueLabel.className = "custom-select-value";
  valueLabel.id = `${select.id || `select-${crypto.randomUUID()}`}-value`;
  const chevron = document.createElement("span");
  chevron.className = "custom-select-chevron";
  chevron.setAttribute("aria-hidden", "true");
  button.append(valueLabel, chevron);

  const listbox = document.createElement("div");
  listbox.className = "custom-select-listbox";
  listbox.id = `${select.id || `select-${crypto.randomUUID()}`}-listbox`;
  listbox.setAttribute("role", "listbox");
  listbox.setAttribute("popover", "manual");
  button.setAttribute("aria-controls", listbox.id);
  const associatedLabel = select.labels?.[0];
  if (associatedLabel !== undefined) {
    if (associatedLabel.id === "") associatedLabel.id = `${listbox.id}-label`;
    button.setAttribute("aria-labelledby", `${associatedLabel.id} ${valueLabel.id}`);
    associatedLabel.addEventListener("click", (event) => {
      event.preventDefault();
      button.focus();
    });
  }

  // Renders (or re-renders) one option's copy from its current text — called again on every sync
  // so a later change to the underlying <option> text (e.g. i18n translations landing after this
  // select was already enhanced) reaches the pre-built dropdown, not just the closed-state label.
  function renderOptionCopy(copy: HTMLSpanElement, text: string): void {
    copy.replaceChildren();
    const [title, ...details] = text.split("·").map((part) => part.trim());
    const titleElement = document.createElement("span");
    titleElement.className = "custom-select-option-title";
    titleElement.textContent = title ?? text;
    copy.append(titleElement);
    if (details.length > 0) {
      const detailElement = document.createElement("span");
      detailElement.className = "custom-select-option-detail";
      detailElement.textContent = details.join(" · ");
      copy.append(detailElement);
    }
  }

  const items = Array.from(select.options).map((option, index) => {
    const item = document.createElement("button");
    item.type = "button";
    item.className = "custom-select-option";
    item.setAttribute("role", "option");
    item.dataset.index = String(index);
    const copy = document.createElement("span");
    copy.className = "custom-select-option-copy";
    renderOptionCopy(copy, option.text);
    const check = document.createElement("span");
    check.className = "custom-select-check";
    check.setAttribute("aria-hidden", "true");
    item.append(copy, check);
    listbox.append(item);
    return { item, copy };
  });

  wrapper.prepend(button);
  wrapper.append(listbox);
  let view: SelectView;

  const close = (restoreFocus = false): void => {
    if (listbox.matches(":popover-open")) listbox.hidePopover();
    button.setAttribute("aria-expanded", "false");
    wrapper.classList.remove("open");
    if (openView === view) openView = undefined;
    if (restoreFocus) button.focus();
  };
  const sync = (): void => {
    const selectedIndex = Math.max(0, select.selectedIndex);
    valueLabel.textContent = select.options[selectedIndex]?.text.split("·")[0]?.trim() ?? "";
    button.disabled = select.disabled;
    button.setAttribute("aria-disabled", String(select.disabled));
    items.forEach(({ item, copy }, index) => {
      const option = select.options[index];
      if (option !== undefined) renderOptionCopy(copy, option.text);
      const selected = index === selectedIndex;
      item.classList.toggle("selected", selected);
      item.setAttribute("aria-selected", String(selected));
      item.disabled = option?.disabled ?? false;
      // Mirrored, not just `disabled`: a level the host cannot honour should be absent from the
      // list rather than present and greyed out, which would read as "temporarily unavailable".
      item.hidden = option?.hidden ?? false;
    });
    if (select.disabled) close();
  };
  view = { button, listbox, sync, close };
  views.set(select, view);

  const open = (): void => {
    if (select.disabled) return;
    openView?.close();
    openView = view;
    const rect = button.getBoundingClientRect();
    const menuWidth = Math.min(Math.max(rect.width, 220), window.innerWidth - 16);
    const estimatedHeight = Math.min(274, items.length * 54 + 12);
    const showAbove = window.innerHeight - rect.bottom < estimatedHeight + 8 && rect.top > window.innerHeight - rect.bottom;
    const left = Math.min(Math.max(8, rect.left), window.innerWidth - menuWidth - 8);
    const top = showAbove ? Math.max(8, rect.top - estimatedHeight - 7) : Math.min(window.innerHeight - estimatedHeight - 8, rect.bottom + 7);
    listbox.style.left = `${left}px`;
    listbox.style.top = `${Math.max(8, top)}px`;
    listbox.style.width = `${menuWidth}px`;
    listbox.showPopover();
    button.setAttribute("aria-expanded", "true");
    wrapper.classList.add("open");
    (items[select.selectedIndex] ?? items[0])?.item.focus();
  };
  const choose = (index: number): void => {
    const option = select.options[index];
    if (option === undefined || option.disabled) return;
    select.value = option.value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
    sync();
    close(true);
  };

  button.addEventListener("click", () => listbox.matches(":popover-open") ? close() : open());
  button.addEventListener("keydown", (event) => {
    if (["ArrowDown", "ArrowUp", "Enter", " "].includes(event.key)) {
      event.preventDefault();
      open();
    }
  });
  items.forEach(({ item }, index) => {
    item.addEventListener("click", () => choose(index));
    item.addEventListener("keydown", (event) => {
      const current = index;
      let target = current;
      if (event.key === "ArrowDown") target = (current + 1) % items.length;
      else if (event.key === "ArrowUp") target = (current - 1 + items.length) % items.length;
      else if (event.key === "Home") target = 0;
      else if (event.key === "End") target = items.length - 1;
      else if (event.key === "Enter" || event.key === " ") { event.preventDefault(); choose(current); return; }
      else if (event.key === "Escape") { event.preventDefault(); close(true); return; }
      else if (event.key === "Tab") { close(); return; }
      else return;
      event.preventDefault();
      items[target]?.item.focus();
    });
  });
  select.addEventListener("change", sync);
  new MutationObserver(sync).observe(select, { attributes: true, childList: true, subtree: true });
  sync();
}

export function syncCustomSelect(select: HTMLSelectElement): void {
  views.get(select)?.sync();
}

/// Shows or hides one option, keeping the enhanced dropdown in step with the underlying select.
///
/// A currently selected option is never hidden: something already set to that value must keep
/// displaying it, or the control would show a value its own list does not offer.
export function setOptionAvailable(
  select: HTMLSelectElement,
  value: string,
  available: boolean,
): void {
  const option = Array.from(select.options).find((item) => item.value === value);
  if (option === undefined) return;
  option.hidden = !available && select.value !== value;
  syncCustomSelect(select);
}

document.addEventListener("pointerdown", (event) => {
  const wrapper = openView?.button.parentElement;
  if (openView !== undefined && wrapper !== null && wrapper !== undefined && !wrapper.contains(event.target as Node)) openView.close();
});
window.addEventListener("blur", () => openView?.close());
