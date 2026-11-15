import {
  ChevronRight,
  Container,
  Copy,
  Download,
  Ellipsis,
  File,
  Folder,
  House,
  Lock,
  Pencil,
  Plus,
  RefreshCw,
  RotateCcw,
  Search,
  Server,
  Settings,
  Terminal,
  Trash2,
  Upload,
  X,
  createElement,
  type IconNode,
} from "lucide";

// Named imports keep the bundle limited to the icons used by the workspace.
const icons: Record<string, IconNode> = {
  folder: Folder,
  file: File,
  chevron: ChevronRight,
  terminal: Terminal,
  plus: Plus,
  search: Search,
  refresh: RefreshCw,
  restore: RotateCcw,
  docker: Container,
  home: House,
  close: X,
  more: Ellipsis,
  trash: Trash2,
  edit: Pencil,
  copy: Copy,
  download: Download,
  upload: Upload,
  connection: Server,
  lock: Lock,
  settings: Settings,
};

const markup = new Map<string, string>();
export function icon(name: string): string {
  const key = Object.prototype.hasOwnProperty.call(icons, name) ? name : "file";
  let svg = markup.get(key);
  if (!svg) {
    svg = createElement(icons[key], {
      class: "ui-icon",
      width: 18,
      height: 18,
      "stroke-width": 1.5,
      "aria-hidden": "true",
      focusable: "false",
    }).outerHTML;
    markup.set(key, svg);
  }
  return svg;
}
