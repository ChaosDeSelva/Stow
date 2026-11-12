// Tree-shaken Font Awesome SVG icons, looked up by the old webfont names so
// markup stays `<Icon name="trash" />`. Add a name here before using it.
import type { IconDefinition } from "@fortawesome/fontawesome-common-types";
import {
  faArrowDown,
  faArrowDownWideShort,
  faArrowUp,
  faArrowUpRightFromSquare,
  faBell,
  faBoxArchive,
  faCalendarDay,
  faCheck,
  faChevronDown,
  faChevronRight,
  faCircleCheck,
  faCircleInfo,
  faClock,
  faCopy,
  faDatabase,
  faDownload,
  faEllipsis,
  faFaceSmile,
  faFileArrowUp,
  faFileExport,
  faFileImport,
  faFileLines,
  faFolderOpen,
  faGear,
  faGripVertical,
  faImage,
  faInbox,
  faKey,
  faKeyboard,
  faLayerGroup,
  faListCheck,
  faMagnifyingGlass,
  faMicrophone,
  faNoteSticky,
  faPaperclip,
  faPen,
  faPlay,
  faPlus,
  faPuzzlePiece,
  faRotate,
  faRotateLeft,
  faShieldHalved,
  faStop,
  faTrash,
  faTriangleExclamation,
  faWandMagicSparkles,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import {
  faCircleCheck as farCircleCheck,
  faCopy as farCopy,
  faCircle as farCircle,
} from "@fortawesome/free-regular-svg-icons";
import { faChrome } from "@fortawesome/free-brands-svg-icons";

export type IconStyle = "solid" | "regular" | "brands";

const solid: Record<string, IconDefinition> = {
  "arrow-down": faArrowDown,
  "arrow-down-wide-short": faArrowDownWideShort,
  "arrow-up": faArrowUp,
  "arrow-up-right-from-square": faArrowUpRightFromSquare,
  bell: faBell,
  "box-archive": faBoxArchive,
  "calendar-day": faCalendarDay,
  check: faCheck,
  "chevron-down": faChevronDown,
  "chevron-right": faChevronRight,
  "circle-check": faCircleCheck,
  "circle-info": faCircleInfo,
  clock: faClock,
  copy: faCopy,
  database: faDatabase,
  download: faDownload,
  ellipsis: faEllipsis,
  "face-smile": faFaceSmile,
  "file-arrow-up": faFileArrowUp,
  "file-export": faFileExport,
  "file-import": faFileImport,
  "file-lines": faFileLines,
  "folder-open": faFolderOpen,
  gear: faGear,
  "grip-vertical": faGripVertical,
  image: faImage,
  inbox: faInbox,
  key: faKey,
  keyboard: faKeyboard,
  "layer-group": faLayerGroup,
  "list-check": faListCheck,
  "magnifying-glass": faMagnifyingGlass,
  microphone: faMicrophone,
  "note-sticky": faNoteSticky,
  paperclip: faPaperclip,
  pen: faPen,
  play: faPlay,
  plus: faPlus,
  "puzzle-piece": faPuzzlePiece,
  rotate: faRotate,
  "rotate-left": faRotateLeft,
  "shield-halved": faShieldHalved,
  stop: faStop,
  trash: faTrash,
  "triangle-exclamation": faTriangleExclamation,
  "wand-magic-sparkles": faWandMagicSparkles,
  xmark: faXmark,
};
const regular: Record<string, IconDefinition> = {
  "circle-check": farCircleCheck,
  copy: farCopy,
  circle: farCircle,
};
const brands: Record<string, IconDefinition> = { chrome: faChrome };

/** `[width, height, path]` for a name, falling back to solid, then to null. */
export function iconData(name: string, style: IconStyle = "solid"): { w: number; h: number; d: string } | null {
  const def = (style === "regular" ? regular[name] : style === "brands" ? brands[name] : undefined) ?? solid[name];
  if (!def) return null;
  const [w, h, , , path] = def.icon;
  return { w, h, d: Array.isArray(path) ? path.join(" ") : path };
}
