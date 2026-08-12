import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open } from "@tauri-apps/plugin-dialog";
import { listen } from "@tauri-apps/api/event";
import { useTranslation } from "react-i18next";
import { getFileIcon } from "../Utils/fileIcons";
import "./ConceptDesktop.css";

type NodeType = "Directory" | "File";

interface VfsNode {
  node_type: NodeType;
  id: number;
  pid: number;
  name: string;
  size: string;
  time: number;
  ext: string;
  chunks: string;
}

interface Breadcrumb {
  id: number;
  name: string;
}

interface ItemPose {
  x: number;
  y: number;
  rotation: number;
  scale: number;
  pinned: boolean;
  z: number;
}

type PileMode = "stack" | "fan" | "grid" | "leaf";

interface VisualPile {
  id: string;
  title: string;
  nodeIds: number[];
  x: number;
  y: number;
  mode: PileMode;
  leafIndex: number;
}

interface SavedDesktop {
  poses: Record<number, ItemPose>;
  piles: VisualPile[];
}

interface DragSession {
  pointerId: number;
  nodeIds: number[];
  lastClientX: number;
  lastClientY: number;
  lastTime: number;
  velocityX: number;
  velocityY: number;
  contacts: Set<string>;
}

interface PileDragSession {
  pointerId: number;
  pileId: string;
  lastClientX: number;
  lastClientY: number;
}

interface LassoState {
  pointerId: number;
  startX: number;
  startY: number;
  currentX: number;
  currentY: number;
  additive: boolean;
  baseline: Set<number>;
}

interface Velocity {
  x: number;
  y: number;
}

interface ContextMenuState {
  x: number;
  y: number;
  targetId: number | null;
}

interface EditDialogState {
  kind: "create" | "rename";
  targetId: number | null;
  name: string;
  description: string;
}

interface MoveDialogState {
  itemIds: number[];
  originalPid: number;
  folders: VfsNode[];
  breadcrumbs: Breadcrumb[];
}

interface NoticeState {
  title: string;
  message: string;
}

const CARD_WIDTH = 120;
const CARD_HEIGHT = 92;
const PILE_WIDTH = 300;
const PILE_HEIGHT = 220;
const HITBOX_GAP = 10;
const SURFACE_TOP = 68;
const STORAGE_VERSION = "heriheri_concept_desktop_v1";

function readSavedDesktop(key: string): SavedDesktop | null {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<SavedDesktop>;
    if (!parsed.poses || !Array.isArray(parsed.piles)) return null;
    return { poses: parsed.poses, piles: parsed.piles };
  } catch {
    return null;
  }
}

function deterministicRotation(id: number): number {
  return ((id * 37) % 13) - 6;
}

function makeDefaultPose(index: number, id: number, width: number): ItemPose {
  const usableWidth = Math.max(320, width - 72);
  const columns = Math.max(2, Math.floor(usableWidth / (CARD_WIDTH + 28)));
  return {
    x: 36 + (index % columns) * (CARD_WIDTH + 28),
    y: 82 + Math.floor(index / columns) * (CARD_HEIGHT + 26),
    rotation: deterministicRotation(id),
    scale: 1,
    pinned: false,
    z: index + 1,
  };
}

interface Bounds {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

function poseBounds(pose: ItemPose): Bounds {
  const width = CARD_WIDTH * pose.scale;
  const height = CARD_HEIGHT * pose.scale;
  const radians = pose.rotation * Math.PI / 180;
  const cosine = Math.cos(radians);
  const sine = Math.sin(radians);
  const corners = [
    { x: 0, y: 0 },
    { x: width * cosine, y: width * sine },
    { x: -height * sine, y: height * cosine },
    { x: width * cosine - height * sine, y: width * sine + height * cosine },
  ];
  const xs = corners.map((corner) => corner.x + pose.x);
  const ys = corners.map((corner) => corner.y + pose.y);
  return { left: Math.min(...xs), right: Math.max(...xs), top: Math.min(...ys), bottom: Math.max(...ys) };
}

function pileBounds(pile: Pick<VisualPile, "x" | "y">): Bounds {
  return { left: pile.x, right: pile.x + PILE_WIDTH, top: pile.y, bottom: pile.y + PILE_HEIGHT };
}

function boundsOverlap(first: Bounds, second: Bounds): boolean {
  return !(
    first.right + HITBOX_GAP <= second.left ||
    second.right + HITBOX_GAP <= first.left ||
    first.bottom + HITBOX_GAP <= second.top ||
    second.bottom + HITBOX_GAP <= first.top
  );
}

function posesOverlap(a: ItemPose, b: ItemPose): boolean {
  return boundsOverlap(poseBounds(a), poseBounds(b));
}

function clampPose(pose: ItemPose, width: number, height: number): ItemPose {
  const maxX = Math.max(0, width - CARD_WIDTH * pose.scale - 12);
  const maxY = Math.max(SURFACE_TOP, height - CARD_HEIGHT * pose.scale - 12);
  return {
    ...pose,
    x: Math.max(0, Math.min(maxX, pose.x)),
    y: pose.pinned ? 12 : Math.max(SURFACE_TOP, Math.min(maxY, pose.y)),
  };
}

function poseIsFree(pose: ItemPose, occupied: ItemPose[], pileObstacles: Bounds[] = []): boolean {
  const bounds = poseBounds(pose);
  return occupied.every((other) => !posesOverlap(pose, other)) && pileObstacles.every((pile) => !boundsOverlap(bounds, pile));
}

function findAvailablePose(preferred: ItemPose, occupied: ItemPose[], width: number, height: number, pileObstacles: Bounds[] = []): ItemPose {
  const clamped = clampPose(preferred, width, height);
  if (poseIsFree(clamped, occupied, pileObstacles)) return clamped;

  const stepX = CARD_WIDTH + HITBOX_GAP + 8;
  const stepY = CARD_HEIGHT + HITBOX_GAP + 8;
  for (let y = SURFACE_TOP; y <= height - CARD_HEIGHT - 10; y += stepY) {
    for (let x = 10; x <= width - CARD_WIDTH - 10; x += stepX) {
      const candidate = { ...clamped, x, y, rotation: 0, pinned: false };
      if (poseIsFree(candidate, occupied, pileObstacles)) return candidate;
    }
  }
  const overflowTop = occupied.reduce((bottom, pose) => Math.max(bottom, poseBounds(pose).bottom), SURFACE_TOP);
  return { ...clamped, x: 10, y: overflowTop + HITBOX_GAP, rotation: 0, pinned: false };
}

function layoutHasCollision(layout: Record<number, ItemPose>, looseIds: number[], pileObstacles: Bounds[] = []): boolean {
  for (let index = 0; index < looseIds.length; index += 1) {
    const first = layout[looseIds[index]];
    if (!first) continue;
    if (pileObstacles.some((pile) => boundsOverlap(poseBounds(first), pile))) return true;
    for (let otherIndex = index + 1; otherIndex < looseIds.length; otherIndex += 1) {
      const second = layout[looseIds[otherIndex]];
      if (second && posesOverlap(first, second)) return true;
    }
  }
  return false;
}

function clampPile(pile: VisualPile, width: number, height: number): VisualPile {
  return {
    ...pile,
    x: Math.max(0, Math.min(Math.max(0, width - PILE_WIDTH - 10), pile.x)),
    y: Math.max(SURFACE_TOP, Math.min(Math.max(SURFACE_TOP, height - PILE_HEIGHT - 10), pile.y)),
  };
}

function pileIsFree(pile: VisualPile, otherPiles: Bounds[], itemBounds: Bounds[]): boolean {
  const bounds = pileBounds(pile);
  return otherPiles.every((other) => !boundsOverlap(bounds, other)) && itemBounds.every((item) => !boundsOverlap(bounds, item));
}

function findAvailablePile(preferred: VisualPile, otherPiles: Bounds[], itemBounds: Bounds[], width: number, height: number): VisualPile {
  const clamped = clampPile(preferred, width, height);
  if (pileIsFree(clamped, otherPiles, itemBounds)) return clamped;
  for (let y = SURFACE_TOP; y <= height - PILE_HEIGHT - 10; y += PILE_HEIGHT + HITBOX_GAP) {
    for (let x = 10; x <= width - PILE_WIDTH - 10; x += PILE_WIDTH + HITBOX_GAP) {
      const candidate = { ...clamped, x, y };
      if (pileIsFree(candidate, otherPiles, itemBounds)) return candidate;
    }
  }
  const overflowTop = [...otherPiles, ...itemBounds].reduce((bottom, bounds) => Math.max(bottom, bounds.bottom), SURFACE_TOP);
  return { ...clamped, x: 10, y: overflowTop + HITBOX_GAP };
}

function parseBytes(value: string): number {
  const normalized = String(value || "0").trim().toUpperCase().replace(/\s/g, "");
  const match = normalized.match(/^([\d.]+)([KMGT]?B?)?$/);
  if (!match) return Number.parseInt(normalized, 10) || 0;
  const amount = Number.parseFloat(match[1]) || 0;
  const unit = match[2]?.replace("B", "") || "";
  const power = ["", "K", "M", "G", "T"].indexOf(unit);
  return amount * Math.pow(1024, Math.max(0, power));
}

function itemMass(node: VfsNode | undefined): number {
  if (!node || node.node_type === "Directory") return 1.15;
  const gigabytes = parseBytes(node.size) / (1024 * 1024 * 1024);
  return Math.min(1.9, 0.72 + Math.log2(1 + gigabytes) * 0.28);
}

function pileCardStyle(mode: PileMode, index: number, count: number, leafIndex: number) {
  if (mode === "fan") {
    const normalized = count <= 1 ? 0 : index / (count - 1) - 0.5;
    return {
      transform: `translate(${normalized * 100}px, ${Math.abs(normalized) * 22}px) rotate(${normalized * 18}deg)`,
      zIndex: index + 1,
    };
  }
  if (mode === "grid") {
    const page = Math.floor(Math.max(0, leafIndex) / 6);
    const relativeIndex = index - page * 6;
    const onPage = relativeIndex >= 0 && relativeIndex < 6;
    const column = relativeIndex % 3;
    const row = Math.floor(relativeIndex / 3);
    return {
      transform: `translate(${column * 76 - 76}px, ${row * 66 - 18}px) scale(.62)`,
      opacity: onPage ? 1 : 0,
      pointerEvents: onPage ? ("auto" as const) : ("none" as const),
      zIndex: index + 1,
    };
  }
  if (mode === "leaf") {
    const delta = index - leafIndex;
    return {
      transform: `translate(${Math.max(-2, Math.min(2, delta)) * 10}px, ${Math.abs(delta) * 3}px) rotate(${delta * 2}deg)`,
      opacity: index === leafIndex ? 1 : 0,
      pointerEvents: index === leafIndex ? ("auto" as const) : ("none" as const),
      zIndex: index === leafIndex ? count + 2 : index,
    };
  }
  return {
    transform: `translate(${Math.min(index, 8) * 4}px, ${Math.min(index, 8) * -4}px) rotate(${(index % 5) - 2}deg)`,
    zIndex: index + 1,
  };
}

export default function ConceptDesktop({ status }: { status: string }) {
  const { t } = useTranslation();
  const roomRef = useRef<HTMLDivElement>(null);
  const layoutRef = useRef<Record<number, ItemPose>>({});
  const velocitiesRef = useRef<Map<number, Velocity>>(new Map());
  const animationRef = useRef<number | null>(null);
  const lastAnimationTimeRef = useRef<number>(0);
  const dragRef = useRef<DragSession | null>(null);
  const pileDragRef = useRef<PileDragSession | null>(null);
  const lassoRef = useRef<LassoState | null>(null);

  const [nodes, setNodes] = useState<VfsNode[]>([]);
  const [breadcrumbs, setBreadcrumbs] = useState<Breadcrumb[]>([]);
  const [currentPid, setCurrentPid] = useState(0);
  const [poses, setPoses] = useState<Record<number, ItemPose>>({});
  const [piles, setPiles] = useState<VisualPile[]>([]);
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [activePileId, setActivePileId] = useState<string | null>(null);
  const [lasso, setLasso] = useState<LassoState | null>(null);
  const [query, setQuery] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [storageKey, setStorageKey] = useState("");
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [clipboard, setClipboard] = useState<{ type: "cut"; ids: number[] } | null>(null);
  const [editDialog, setEditDialog] = useState<EditDialogState | null>(null);
  const [moveDialog, setMoveDialog] = useState<MoveDialogState | null>(null);
  const [deleteDialogIds, setDeleteDialogIds] = useState<number[] | null>(null);
  const [showResetDialog, setShowResetDialog] = useState(false);
  const [notice, setNotice] = useState<NoticeState | null>(null);

  const nodeMap = useMemo(() => new Map(nodes.map((node) => [node.id, node])), [nodes]);
  const piledIds = useMemo(() => new Set(piles.flatMap((pile) => pile.nodeIds)), [piles]);
  const visibleNodes = useMemo(() => {
    const needle = query.trim().toLowerCase();
    return nodes.filter((node) => !piledIds.has(node.id) && (!needle || node.name.toLowerCase().includes(needle)));
  }, [nodes, piledIds, query]);

  const updatePoses = useCallback((updater: (current: Record<number, ItemPose>) => Record<number, ItemPose>) => {
    setPoses((current) => {
      const next = updater(current);
      layoutRef.current = next;
      return next;
    });
  }, []);

  const hydrateSurface = useCallback((data: VfsNode[], pid: number) => {
    const phone = localStorage.getItem("phone") || "guest";
    const key = `${STORAGE_VERSION}:${phone}:${pid}`;
    const saved = readSavedDesktop(key);
    const width = roomRef.current?.clientWidth || window.innerWidth - 360;
    const height = roomRef.current?.clientHeight || 620;
    const nextPoses: Record<number, ItemPose> = {};
    const ids = new Set(data.map((node) => node.id));
    const restoredPiles = (saved?.piles || [])
      .map((pile) => ({ ...pile, nodeIds: pile.nodeIds.filter((id) => ids.has(id)) }))
      .filter((pile) => pile.nodeIds.length > 1);
    const nextPiles: VisualPile[] = [];
    restoredPiles.forEach((pile) => {
      nextPiles.push(findAvailablePile(pile, nextPiles.map(pileBounds), [], width, height));
    });
    const hiddenIds = new Set(nextPiles.flatMap((pile) => pile.nodeIds));
    const occupied: ItemPose[] = [];
    const pileObstacles = nextPiles.map(pileBounds);

    data.forEach((node, index) => {
      const candidate = saved?.poses?.[node.id];
      const preferred = candidate && Number.isFinite(candidate.x) && Number.isFinite(candidate.y)
        ? { ...candidate, scale: candidate.scale || 1, z: candidate.z || index + 1 }
        : makeDefaultPose(index, node.id, width);
      nextPoses[node.id] = hiddenIds.has(node.id)
        ? clampPose(preferred, width, height)
        : findAvailablePose(preferred, occupied, width, height, pileObstacles);
      if (!hiddenIds.has(node.id)) occupied.push(nextPoses[node.id]);
    });

    layoutRef.current = nextPoses;
    setStorageKey(key);
    setPoses(nextPoses);
    setPiles(nextPiles);
    setSelected(new Set());
    setActivePileId(null);
    setContextMenu(null);
  }, []);

  const fetchDirectory = useCallback(async (allowInitialize = true) => {
    setLoading(true);
    setError("");
    try {
      let data: VfsNode[];
      try {
        data = await invoke<VfsNode[]>("vfs_list_dir");
      } catch (initialError) {
        if (!allowInitialize || status !== "Connected") throw initialError;
        await invoke("init_vfs_root", { phone: localStorage.getItem("phone") || "" });
        await invoke("vfs_sync_pull").catch(() => false);
        data = await invoke<VfsNode[]>("vfs_list_dir");
      }
      const [path, pid] = await Promise.all([
        invoke<Breadcrumb[]>("vfs_get_breadcrumbs").catch(() => []),
        invoke<number>("vfs_get_current_pid").catch(() => 0),
      ]);
      setNodes(data);
      setBreadcrumbs(path);
      setCurrentPid(pid);
      hydrateSurface(data, pid);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setLoading(false);
    }
  }, [hydrateSurface, status]);

  useEffect(() => {
    fetchDirectory();
    return () => {
      if (animationRef.current !== null) cancelAnimationFrame(animationRef.current);
    };
  }, [fetchDirectory]);

  useEffect(() => {
    if (!storageKey) return;
    const timer = window.setTimeout(() => {
      const payload: SavedDesktop = { poses, piles };
      localStorage.setItem(storageKey, JSON.stringify(payload));
    }, 180);
    return () => window.clearTimeout(timer);
  }, [poses, piles, storageKey]);

  useEffect(() => {
    const closeContextMenu = () => setContextMenu(null);
    window.addEventListener("pointerdown", closeContextMenu);
    window.addEventListener("blur", closeContextMenu);
    return () => {
      window.removeEventListener("pointerdown", closeContextMenu);
      window.removeEventListener("blur", closeContextMenu);
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<{ paths?: string[] }>("tauri://drag-drop", async (event) => {
      if (status !== "Connected") return;
      const paths = event.payload.paths || [];
      if (paths.length === 0) return;
      setLoading(true);
      setNotice({ title: t("Processing"), message: t("Scanning folder structure... Please wait.") });
      try {
        const expanded = await invoke<Array<{ path: string; targetPid: number; groupId?: string; groupName?: string }>>("vfs_expand_drop", { paths, currentPid });
        const active = JSON.parse(localStorage.getItem("heriheri_active") || "[]");
        const groups = new Map<string, { id: string; isGroup: boolean; name: string; status: string; totalItems: number; finishedItems: number; type: string }>();
        expanded.forEach((file) => {
          if (!file.groupId) return;
          const group = groups.get(file.groupId) || { id: file.groupId, isGroup: true, name: file.groupName || t("Folder"), status: "Queued", totalItems: 0, finishedItems: 0, type: "Upload" };
          group.totalItems += 1;
          groups.set(file.groupId, group);
        });
        groups.forEach((group) => active.push(group));
        expanded.forEach((file) => active.push({
          id: `t_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
          groupId: file.groupId,
          isGroup: false,
          name: file.path.split(/[/\\]/).pop() || t("Unknown File"),
          type: "Upload",
          status: "Queued",
          filePath: file.path,
          targetPid: file.targetPid,
          resumeFolder: "",
          resumeChunk: 0,
        }));
        if (expanded.length > 0) {
          localStorage.setItem("heriheri_active", JSON.stringify(active));
          window.dispatchEvent(new CustomEvent("TASK_START"));
          setNotice({ title: t("Upload Queued"), message: `${expanded.length} ${t("items added to the queue.")}` });
        } else {
          setNotice(null);
        }
      } catch (reason) {
        setNotice({ title: t("Drop Error"), message: t("Failed to process folder: ") + String(reason) });
      } finally {
        setLoading(false);
      }
    });
    return () => { unlisten.then((dispose) => dispose()); };
  }, [currentPid, status, t]);

  const startPhysics = useCallback(() => {
    if (animationRef.current !== null) return;
    lastAnimationTimeRef.current = performance.now();

    const tick = (now: number) => {
      const elapsed = Math.min(0.032, Math.max(0.001, (now - lastAnimationTimeRef.current) / 1000));
      lastAnimationTimeRef.current = now;
      const room = roomRef.current;
      if (!room) {
        animationRef.current = null;
        return;
      }

      const next = { ...layoutRef.current };
      const looseIds = Object.keys(next).map(Number).filter((id) => !piledIds.has(id));
      const draggedIds = new Set(dragRef.current?.nodeIds || []);
      const pileObstacles = piles.map(pileBounds);
      const frameVelocities = new Map<number, Velocity>();

      looseIds.forEach((id) => {
        const pose = next[id];
        const velocity = velocitiesRef.current.get(id) || { x: 0, y: 0 };
        if (!pose || pose.pinned || draggedIds.has(id)) {
          frameVelocities.set(id, { x: 0, y: 0 });
          return;
        }
        const rawX = pose.x + velocity.x * elapsed;
        const rawY = pose.y + velocity.y * elapsed;
        let candidate = clampPose({ ...pose, x: rawX, y: rawY }, room.clientWidth, room.clientHeight);
        let velocityX = velocity.x;
        let velocityY = velocity.y;
        if (candidate.x !== rawX) velocityX *= -0.52;
        if (candidate.y !== rawY) velocityY *= -0.52;
        pileObstacles.forEach((pile) => {
          const card = poseBounds(candidate);
          if (!boundsOverlap(card, pile)) return;
          const overlapX = Math.min(card.right, pile.right) - Math.max(card.left, pile.left) + HITBOX_GAP;
          const overlapY = Math.min(card.bottom, pile.bottom) - Math.max(card.top, pile.top) + HITBOX_GAP;
          if (overlapX < overlapY) {
            const direction = (card.left + card.right) / 2 < (pile.left + pile.right) / 2 ? -1 : 1;
            candidate = clampPose({ ...candidate, x: candidate.x + direction * overlapX }, room.clientWidth, room.clientHeight);
            velocityX *= -0.62;
          } else {
            const direction = (card.top + card.bottom) / 2 < (pile.top + pile.bottom) / 2 ? -1 : 1;
            candidate = clampPose({ ...candidate, y: candidate.y + direction * overlapY }, room.clientWidth, room.clientHeight);
            velocityY *= -0.62;
          }
        });
        next[id] = { ...candidate, rotation: candidate.rotation + velocityX * elapsed * 0.01 };
        frameVelocities.set(id, { x: velocityX, y: velocityY });
      });

      // Resolve collisions as rigid-body impulses. The impulse is equal and opposite,
      // so linear momentum is conserved before restitution and floor friction losses.
      for (let pass = 0; pass < 2; pass += 1) {
        for (let firstIndex = 0; firstIndex < looseIds.length; firstIndex += 1) {
          const firstId = looseIds[firstIndex];
          for (let secondIndex = firstIndex + 1; secondIndex < looseIds.length; secondIndex += 1) {
            const firstPose = next[firstId];
            const secondId = looseIds[secondIndex];
            const secondPose = next[secondId];
            if (draggedIds.has(firstId) || draggedIds.has(secondId)) continue;
            if (!firstPose || !secondPose || !posesOverlap(firstPose, secondPose)) continue;

            const firstBounds = poseBounds(firstPose);
            const secondBounds = poseBounds(secondPose);
            const overlapX = Math.min(firstBounds.right, secondBounds.right) - Math.max(firstBounds.left, secondBounds.left) + HITBOX_GAP;
            const overlapY = Math.min(firstBounds.bottom, secondBounds.bottom) - Math.max(firstBounds.top, secondBounds.top) + HITBOX_GAP;
            const firstCenterX = (firstBounds.left + firstBounds.right) / 2;
            const firstCenterY = (firstBounds.top + firstBounds.bottom) / 2;
            const secondCenterX = (secondBounds.left + secondBounds.right) / 2;
            const secondCenterY = (secondBounds.top + secondBounds.bottom) / 2;
            const useHorizontalNormal = overlapX < overlapY;
            const normalX = useHorizontalNormal ? (secondCenterX >= firstCenterX ? 1 : -1) : 0;
            const normalY = useHorizontalNormal ? 0 : (secondCenterY >= firstCenterY ? 1 : -1);
            const penetration = Math.max(0, useHorizontalNormal ? overlapX : overlapY);
            const inverseFirstMass = firstPose.pinned ? 0 : 1 / itemMass(nodeMap.get(firstId));
            const inverseSecondMass = secondPose.pinned ? 0 : 1 / itemMass(nodeMap.get(secondId));
            const inverseMassSum = inverseFirstMass + inverseSecondMass;
            if (inverseMassSum === 0) continue;

            const correction = Math.max(0, penetration - 0.5) * 0.72 / inverseMassSum;
            if (!firstPose.pinned) {
              next[firstId] = clampPose({
                ...firstPose,
                x: firstPose.x - normalX * correction * inverseFirstMass,
                y: firstPose.y - normalY * correction * inverseFirstMass,
              }, room.clientWidth, room.clientHeight);
            }
            if (!secondPose.pinned) {
              next[secondId] = clampPose({
                ...secondPose,
                x: secondPose.x + normalX * correction * inverseSecondMass,
                y: secondPose.y + normalY * correction * inverseSecondMass,
              }, room.clientWidth, room.clientHeight);
            }

            const firstVelocity = frameVelocities.get(firstId) || { x: 0, y: 0 };
            const secondVelocity = frameVelocities.get(secondId) || { x: 0, y: 0 };
            const closingSpeed = (secondVelocity.x - firstVelocity.x) * normalX + (secondVelocity.y - firstVelocity.y) * normalY;
            if (closingSpeed >= 0) continue;
            const restitution = 0.68;
            const impulse = -(1 + restitution) * closingSpeed / inverseMassSum;
            frameVelocities.set(firstId, {
              x: firstVelocity.x - impulse * inverseFirstMass * normalX,
              y: firstVelocity.y - impulse * inverseFirstMass * normalY,
            });
            frameVelocities.set(secondId, {
              x: secondVelocity.x + impulse * inverseSecondMass * normalX,
              y: secondVelocity.y + impulse * inverseSecondMass * normalY,
            });
          }
        }
      }

      let hasMotion = false;
      const friction = Math.pow(0.93, elapsed * 60);
      looseIds.forEach((id) => {
        const pose = next[id];
        const velocity = frameVelocities.get(id) || { x: 0, y: 0 };
        if (!pose || pose.pinned || draggedIds.has(id)) {
          velocitiesRef.current.delete(id);
          return;
        }
        const slowed = { x: velocity.x * friction, y: velocity.y * friction };
        if (Math.hypot(slowed.x, slowed.y) < 7) {
          velocitiesRef.current.delete(id);
        } else {
          velocitiesRef.current.set(id, slowed);
          hasMotion = true;
        }
      });

      layoutRef.current = next;
      setPoses(next);
      if (hasMotion) {
        animationRef.current = requestAnimationFrame(tick);
      } else {
        animationRef.current = null;
      }
    };

    animationRef.current = requestAnimationFrame(tick);
  }, [nodeMap, piledIds, piles]);

  const bringToFront = useCallback((ids: number[]) => {
    updatePoses((current) => {
      const next = { ...current };
      let top = Math.max(0, ...Object.values(current).map((pose) => pose.z));
      ids.forEach((id) => {
        if (next[id]) next[id] = { ...next[id], z: ++top };
      });
      return next;
    });
  }, [updatePoses]);

  const handleItemPointerDown = (event: React.PointerEvent<HTMLDivElement>, id: number) => {
    if (event.button !== 0) return;
    event.stopPropagation();
    const nextSelected = new Set(event.ctrlKey || event.metaKey ? selected : []);
    nextSelected.add(id);
    setSelected(nextSelected);
    setActivePileId(null);
    const movingIds = Array.from(nextSelected).filter((nodeId) => !piledIds.has(nodeId));
    movingIds.forEach((nodeId) => velocitiesRef.current.delete(nodeId));
    bringToFront(movingIds);
    dragRef.current = {
      pointerId: event.pointerId,
      nodeIds: movingIds,
      lastClientX: event.clientX,
      lastClientY: event.clientY,
      lastTime: performance.now(),
      velocityX: 0,
      velocityY: 0,
      contacts: new Set(),
    };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handleItemPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const room = roomRef.current;
    if (!room) return;
    const now = performance.now();
    const elapsed = Math.max(8, now - drag.lastTime);
    const dx = event.clientX - drag.lastClientX;
    const dy = event.clientY - drag.lastClientY;
    drag.velocityX = (dx / elapsed) * 1000;
    drag.velocityY = (dy / elapsed) * 1000;
    const measuredSpeed = Math.hypot(drag.velocityX, drag.velocityY);
    if (measuredSpeed > 1800) {
      const scale = 1800 / measuredSpeed;
      drag.velocityX *= scale;
      drag.velocityY *= scale;
    }
    drag.lastClientX = event.clientX;
    drag.lastClientY = event.clientY;
    drag.lastTime = now;
    const current = layoutRef.current;
    const pileObstacles = piles.map(pileBounds);
    const draggedIds = new Set(drag.nodeIds);
    const looseIds = Object.keys(current).map(Number).filter((id) => !piledIds.has(id));
    const tryMove = (moveX: number, moveY: number) => {
      const candidate = { ...current };
      for (const id of drag.nodeIds) {
        const pose = candidate[id];
        if (!pose) continue;
        const moved = clampPose({ ...pose, x: pose.x + moveX, y: pose.y + moveY }, room.clientWidth, room.clientHeight);
        if (pileObstacles.some((pile) => boundsOverlap(poseBounds(moved), pile))) return null;
        candidate[id] = moved;
      }
      return candidate;
    };
    const next = tryMove(dx, dy) || tryMove(dx, 0) || tryMove(0, dy);
    if (!next) return;

    const nextContacts = new Set<string>();
    let causedImpact = false;
    for (const sourceId of drag.nodeIds) {
      const sourcePose = next[sourceId];
      if (!sourcePose) continue;
      for (const targetId of looseIds) {
        if (draggedIds.has(targetId)) continue;
        const targetPose = next[targetId];
        if (!targetPose || !posesOverlap(sourcePose, targetPose)) continue;

        const contactKey = `${sourceId}:${targetId}`;
        nextContacts.add(contactKey);
        const sourceBounds = poseBounds(sourcePose);
        const targetBounds = poseBounds(targetPose);
        const overlapX = Math.min(sourceBounds.right, targetBounds.right) - Math.max(sourceBounds.left, targetBounds.left) + HITBOX_GAP;
        const overlapY = Math.min(sourceBounds.bottom, targetBounds.bottom) - Math.max(sourceBounds.top, targetBounds.top) + HITBOX_GAP;
        const sourceCenterX = (sourceBounds.left + sourceBounds.right) / 2;
        const sourceCenterY = (sourceBounds.top + sourceBounds.bottom) / 2;
        const targetCenterX = (targetBounds.left + targetBounds.right) / 2;
        const targetCenterY = (targetBounds.top + targetBounds.bottom) / 2;
        const horizontal = overlapX < overlapY;
        const normalX = horizontal ? (targetCenterX >= sourceCenterX ? 1 : -1) : 0;
        const normalY = horizontal ? 0 : (targetCenterY >= sourceCenterY ? 1 : -1);
        const penetration = Math.max(0, horizontal ? overlapX : overlapY);

        if (!targetPose.pinned) {
          const separated = clampPose({
            ...targetPose,
            x: targetPose.x + normalX * penetration,
            y: targetPose.y + normalY * penetration,
          }, room.clientWidth, room.clientHeight);
          if (!pileObstacles.some((pile) => boundsOverlap(poseBounds(separated), pile))) next[targetId] = separated;
        }

        if (drag.contacts.has(contactKey)) continue;
        const targetVelocity = velocitiesRef.current.get(targetId) || { x: 0, y: 0 };
        const approachingSpeed = (drag.velocityX - targetVelocity.x) * normalX + (drag.velocityY - targetVelocity.y) * normalY;
        if (approachingSpeed <= 0) continue;
        const inverseSourceMass = 1 / itemMass(nodeMap.get(sourceId));
        const inverseTargetMass = targetPose.pinned ? 0 : 1 / itemMass(nodeMap.get(targetId));
        const inverseMassSum = inverseSourceMass + inverseTargetMass;
        if (inverseMassSum === 0) continue;
        const restitution = 0.72;
        const impulse = (1 + restitution) * approachingSpeed / inverseMassSum;
        if (!targetPose.pinned) {
          velocitiesRef.current.set(targetId, {
            x: targetVelocity.x + impulse * inverseTargetMass * normalX,
            y: targetVelocity.y + impulse * inverseTargetMass * normalY,
          });
          causedImpact = true;
        }
        drag.velocityX -= impulse * inverseSourceMass * normalX;
        drag.velocityY -= impulse * inverseSourceMass * normalY;
      }
    }
    drag.contacts = nextContacts;
    layoutRef.current = next;
    setPoses(next);
    if (causedImpact) startPhysics();
  };

  const finishItemDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const speed = Math.hypot(drag.velocityX, drag.velocityY);
    const velocityLimit = 1800;
    const velocityScale = speed > velocityLimit ? velocityLimit / speed : 1;
    drag.nodeIds.forEach((id) => {
      const mass = itemMass(nodeMap.get(id));
      velocitiesRef.current.set(id, {
        x: drag.velocityX * velocityScale / mass,
        y: drag.velocityY * velocityScale / mass,
      });
    });
    dragRef.current = null;
    startPhysics();
  };

  const handleRoomPointerDown = (event: React.PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || (event.target as HTMLElement).closest(".concept-item, .concept-pile, .concept-toolbar")) return;
    const rect = roomRef.current?.getBoundingClientRect();
    if (!rect) return;
    const next: LassoState = {
      pointerId: event.pointerId,
      startX: event.clientX - rect.left,
      startY: event.clientY - rect.top,
      currentX: event.clientX - rect.left,
      currentY: event.clientY - rect.top,
      additive: event.ctrlKey || event.metaKey,
      baseline: new Set(event.ctrlKey || event.metaKey ? selected : []),
    };
    lassoRef.current = next;
    setLasso(next);
    setActivePileId(null);
    if (!next.additive) setSelected(new Set());
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handleRoomPointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const current = lassoRef.current;
    const room = roomRef.current;
    if (!current || !room || current.pointerId !== event.pointerId) return;
    const rect = room.getBoundingClientRect();
    const next = { ...current, currentX: event.clientX - rect.left, currentY: event.clientY - rect.top };
    lassoRef.current = next;
    setLasso(next);

    const left = Math.min(next.startX, next.currentX) + rect.left;
    const right = Math.max(next.startX, next.currentX) + rect.left;
    const top = Math.min(next.startY, next.currentY) + rect.top;
    const bottom = Math.max(next.startY, next.currentY) + rect.top;
    const hit = new Set(next.baseline);
    room.querySelectorAll<HTMLElement>("[data-concept-node-id]").forEach((element) => {
      const box = element.getBoundingClientRect();
      if (!(box.right < left || box.left > right || box.bottom < top || box.top > bottom)) {
        hit.add(Number(element.dataset.conceptNodeId));
      }
    });
    setSelected(hit);
  };

  const finishLasso = (event: React.PointerEvent<HTMLDivElement>) => {
    if (lassoRef.current?.pointerId !== event.pointerId) return;
    lassoRef.current = null;
    setLasso(null);
  };

  const tidy = () => {
    const targets = (selected.size > 0 ? Array.from(selected) : visibleNodes.map((node) => node.id))
      .filter((id) => !piledIds.has(id));
    const roomWidth = roomRef.current?.clientWidth || 900;
    const roomHeight = roomRef.current?.clientHeight || 620;
    const targetSet = new Set(targets);
    const pileObstacles = piles.map(pileBounds);
    updatePoses((current) => {
      const next = { ...current };
      const occupied = Object.entries(current)
        .filter(([id]) => !targetSet.has(Number(id)) && !piledIds.has(Number(id)))
        .map(([, pose]) => pose);
      targets.forEach((id, index) => {
        if (!next[id]) return;
        const preferred = {
          ...next[id],
          x: 36 + (index % Math.max(2, Math.floor((roomWidth - 60) / (CARD_WIDTH + 22)))) * (CARD_WIDTH + 22),
          y: 82 + Math.floor(index / Math.max(2, Math.floor((roomWidth - 60) / (CARD_WIDTH + 22)))) * (CARD_HEIGHT + 20),
          rotation: 0,
        };
        next[id] = findAvailablePose(preferred, occupied, roomWidth, roomHeight, pileObstacles);
        occupied.push(next[id]);
      });
      return next;
    });
  };

  const scatter = () => {
    const targets = selected.size > 0 ? Array.from(selected) : visibleNodes.map((node) => node.id);
    const width = Math.max(320, (roomRef.current?.clientWidth || 900) - CARD_WIDTH - 36);
    const height = Math.max(220, (roomRef.current?.clientHeight || 620) - CARD_HEIGHT - 90);
    const roomWidth = roomRef.current?.clientWidth || 900;
    const roomHeight = roomRef.current?.clientHeight || 620;
    const targetSet = new Set(targets);
    const pileObstacles = piles.map(pileBounds);
    updatePoses((current) => {
      const next = { ...current };
      const occupied = Object.entries(current)
        .filter(([id]) => !targetSet.has(Number(id)) && !piledIds.has(Number(id)))
        .map(([, pose]) => pose);
      targets.forEach((id, index) => {
        if (!next[id]) return;
        const preferred = {
          ...next[id],
          x: 24 + ((id * 97 + index * 41) % width),
          y: 68 + ((id * 53 + index * 67) % height),
          rotation: ((id * 29 + index * 11) % 25) - 12,
        };
        next[id] = findAvailablePose(preferred, occupied, roomWidth, roomHeight, pileObstacles);
        occupied.push(next[id]);
      });
      return next;
    });
  };

  const resizeSelected = (delta: number) => {
    updatePoses((current) => {
      const next = { ...current };
      selected.forEach((id) => {
        if (!next[id] || piledIds.has(id)) return;
        const candidate = { ...next[id], scale: Math.max(0.72, Math.min(1.55, next[id].scale + delta)) };
        const looseIds = Object.keys(next).map(Number).filter((nodeId) => !piledIds.has(nodeId));
        if (!layoutHasCollision({ ...next, [id]: candidate }, looseIds, piles.map(pileBounds))) next[id] = candidate;
      });
      return next;
    });
  };

  const togglePinned = () => {
    const roomWidth = roomRef.current?.clientWidth || 900;
    const roomHeight = roomRef.current?.clientHeight || 620;
    updatePoses((current) => {
      const next = { ...current };
      selected.forEach((id) => {
        if (!next[id]) return;
        const pinned = !next[id].pinned;
        const occupied = Object.entries(next)
          .filter(([otherId]) => Number(otherId) !== id && !piledIds.has(Number(otherId)))
          .map(([, pose]) => pose);
        if (!pinned) {
          next[id] = findAvailablePose({ ...next[id], pinned: false, y: Math.max(SURFACE_TOP, next[id].y), rotation: 0 }, occupied, roomWidth, roomHeight, piles.map(pileBounds));
          return;
        }
        for (let x = 10; x <= roomWidth - CARD_WIDTH - 10; x += CARD_WIDTH + HITBOX_GAP) {
          const candidate = clampPose({ ...next[id], x, y: 12, pinned: true, rotation: 0 }, roomWidth, roomHeight);
          if (poseIsFree(candidate, occupied, piles.map(pileBounds))) {
            next[id] = candidate;
            return;
          }
        }
      });
      return next;
    });
  };

  const makePile = () => {
    const ids = Array.from(selected).filter((id) => !piledIds.has(id));
    if (ids.length < 2) return;
    const center = ids.reduce((sum, id) => {
      const pose = poses[id];
      return { x: sum.x + (pose?.x || 0), y: sum.y + (pose?.y || 0) };
    }, { x: 0, y: 0 });
    const preferred: VisualPile = {
      id: `pile_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
      title: `${t("Pile")} ${piles.length + 1}`,
      nodeIds: ids,
      x: center.x / ids.length,
      y: center.y / ids.length,
      mode: "stack",
      leafIndex: ids.length - 1,
    };
    const roomWidth = roomRef.current?.clientWidth || 900;
    const roomHeight = roomRef.current?.clientHeight || 620;
    const remainingItemBounds = Object.entries(poses)
      .filter(([id]) => !ids.includes(Number(id)) && !piledIds.has(Number(id)))
      .map(([, pose]) => poseBounds(pose));
    const pile = findAvailablePile(preferred, piles.map(pileBounds), remainingItemBounds, roomWidth, roomHeight);
    setPiles((current) => [...current, pile]);
    setSelected(new Set());
    setActivePileId(pile.id);
  };

  const updatePile = (id: string, patch: Partial<VisualPile>) => {
    setPiles((current) => current.map((pile) => pile.id === id ? { ...pile, ...patch } : pile));
  };

  const promotePileItem = (pile: VisualPile, nodeId: number) => {
    const index = Math.max(0, pile.nodeIds.indexOf(nodeId));
    if (pile.mode === "leaf") {
      updatePile(pile.id, { leafIndex: index });
      return;
    }
    if (pile.mode === "grid") {
      updatePile(pile.id, { leafIndex: Math.floor(index / 6) * 6 });
      return;
    }
    updatePile(pile.id, { nodeIds: [...pile.nodeIds.filter((id) => id !== nodeId), nodeId] });
  };

  const leafPile = (pile: VisualPile, direction: number) => {
    if (pile.nodeIds.length === 0) return;
    updatePile(pile.id, { mode: "leaf", leafIndex: (pile.leafIndex + direction + pile.nodeIds.length) % pile.nodeIds.length });
  };

  const cycleStackPile = (pile: VisualPile, direction: number) => {
    if (pile.nodeIds.length < 2) return;
    const nodeIds = [...pile.nodeIds];
    if (direction > 0) nodeIds.unshift(nodeIds.pop()!);
    else nodeIds.push(nodeIds.shift()!);
    updatePile(pile.id, { nodeIds });
  };

  const pageGridPile = (pile: VisualPile, direction: number) => {
    const pageCount = Math.max(1, Math.ceil(pile.nodeIds.length / 6));
    const currentPage = Math.floor(Math.max(0, pile.leafIndex) / 6);
    const nextPage = (currentPage + direction + pageCount) % pageCount;
    updatePile(pile.id, { leafIndex: nextPage * 6 });
  };

  const unpile = (pile: VisualPile) => {
    const roomWidth = roomRef.current?.clientWidth || 900;
    const roomHeight = roomRef.current?.clientHeight || 620;
    updatePoses((current) => {
      const next = { ...current };
      const occupied = Object.entries(current)
        .filter(([id]) => !pile.nodeIds.includes(Number(id)) && !piledIds.has(Number(id)))
        .map(([, pose]) => pose);
      const pileObstacles = piles.filter((candidate) => candidate.id !== pile.id).map(pileBounds);
      pile.nodeIds.forEach((id, index) => {
        const x = Math.min(roomWidth - CARD_WIDTH - 16, Math.max(12, pile.x + (index - (pile.nodeIds.length - 1) / 2) * 54));
        const preferred = { ...(next[id] || makeDefaultPose(index, id, roomWidth)), x, y: pile.y + Math.abs(index - 1) * 18 };
        next[id] = findAvailablePose(preferred, occupied, roomWidth, roomHeight, pileObstacles);
        occupied.push(next[id]);
      });
      return next;
    });
    setPiles((current) => current.filter((candidate) => candidate.id !== pile.id));
    setActivePileId(null);
  };

  const handlePilePointerDown = (event: React.PointerEvent<HTMLDivElement>, pileId: string) => {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button, .concept-pile-card")) return;
    event.stopPropagation();
    setActivePileId(pileId);
    setSelected(new Set());
    pileDragRef.current = { pointerId: event.pointerId, pileId, lastClientX: event.clientX, lastClientY: event.clientY };
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handlePilePointerMove = (event: React.PointerEvent<HTMLDivElement>) => {
    const drag = pileDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    const room = roomRef.current;
    if (!room) return;
    const dx = event.clientX - drag.lastClientX;
    const dy = event.clientY - drag.lastClientY;
    drag.lastClientX = event.clientX;
    drag.lastClientY = event.clientY;
    setPiles((current) => {
      const moving = current.find((pile) => pile.id === drag.pileId);
      if (!moving) return current;
      const itemBounds = Object.entries(layoutRef.current)
        .filter(([id]) => !piledIds.has(Number(id)))
        .map(([, pose]) => poseBounds(pose));
      const otherPiles = current.filter((pile) => pile.id !== drag.pileId).map(pileBounds);
      const tryMove = (moveX: number, moveY: number) => {
        const candidate = clampPile({ ...moving, x: moving.x + moveX, y: moving.y + moveY }, room.clientWidth, room.clientHeight);
        return pileIsFree(candidate, otherPiles, itemBounds) ? candidate : null;
      };
      const candidate = tryMove(dx, dy) || tryMove(dx, 0) || tryMove(0, dy);
      return candidate ? current.map((pile) => pile.id === drag.pileId ? candidate : pile) : current;
    });
  };

  const finishPileDrag = (event: React.PointerEvent<HTMLDivElement>) => {
    if (pileDragRef.current?.pointerId === event.pointerId) pileDragRef.current = null;
  };

  const enterFolder = async (id: number) => {
    await invoke("vfs_enter_folder", { id });
    await fetchDirectory(false);
  };

  const goBack = async () => {
    await invoke("vfs_go_back");
    await fetchDirectory(false);
  };

  const jumpToBreadcrumb = async (id: number) => {
    if (id === currentPid) return;
    await invoke("vfs_enter_folder", { id });
    await fetchDirectory(false);
  };

  const refresh = async () => {
    setLoading(true);
    await invoke("vfs_sync_pull").catch(() => false);
    await fetchDirectory(false);
  };

  const contextIds = (targetId: number | null) => {
    if (targetId === null) return [];
    return selected.has(targetId) ? Array.from(selected) : [targetId];
  };

  const openContextMenu = (event: React.MouseEvent, targetId: number | null) => {
    event.preventDefault();
    event.stopPropagation();
    if (targetId !== null && !selected.has(targetId)) setSelected(new Set([targetId]));
    setActivePileId(null);
    setContextMenu({ x: event.clientX, y: event.clientY, targetId });
  };

  const createFolder = async () => {
    setContextMenu(null);
    setEditDialog({ kind: "create", targetId: null, name: "", description: "" });
  };

  const submitEditDialog = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!editDialog || !editDialog.name.trim()) return;
    const pending = editDialog;
    setEditDialog(null);
    try {
      setLoading(true);
      await invoke("vfs_sync_pull").catch(() => false);
      if (pending.kind === "create") {
        await invoke("vfs_create_folder", { name: pending.name.trim(), desc: pending.description.trim() });
      } else if (pending.targetId !== null) {
        await invoke("vfs_rename_item", { id: pending.targetId, newName: pending.name.trim() });
      }
      await invoke("vfs_sync_push").catch(() => false);
      await fetchDirectory(false);
    } catch (reason) {
      setNotice({ title: pending.kind === "create" ? t("Error") : t("Rename Error"), message: String(reason) });
      setLoading(false);
    }
  };

  const uploadFiles = async () => {
    setContextMenu(null);
    try {
      const picked = await open({ multiple: true, title: t("Select Files to Upload") });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      if (paths.length === 0) return;
      const active = JSON.parse(localStorage.getItem("heriheri_active") || "[]");
      paths.forEach((filePath) => {
        active.push({
          id: `t_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
          isGroup: false,
          name: filePath.split(/[/\\]/).pop() || t("Unknown File"),
          type: "Upload",
          status: "Queued",
          filePath,
          targetPid: currentPid,
          resumeFolder: "",
          resumeChunk: 0,
        });
      });
      localStorage.setItem("heriheri_active", JSON.stringify(active));
      window.dispatchEvent(new CustomEvent("TASK_START"));
    } catch (reason) {
      setNotice({ title: t("Upload Error"), message: String(reason) });
    }
  };

  const markForMove = (targetId: number | null) => {
    const ids = contextIds(targetId);
    if (ids.length === 0) return;
    setClipboard({ type: "cut", ids });
    setContextMenu(null);
  };

  const pasteItems = async () => {
    setContextMenu(null);
    if (!clipboard || clipboard.ids.length === 0) return;
    try {
      setLoading(true);
      await invoke("vfs_sync_pull").catch(() => false);
      await invoke("vfs_move_items", { itemIds: clipboard.ids, targetPid: currentPid });
      await invoke("vfs_sync_push").catch(() => false);
      setClipboard(null);
      await fetchDirectory(false);
    } catch (reason) {
      setNotice({ title: t("Move Error"), message: String(reason) });
      setLoading(false);
    }
  };

  const renameItem = async (targetId: number | null) => {
    setContextMenu(null);
    if (targetId === null) return;
    const node = nodeMap.get(targetId);
    setEditDialog({ kind: "rename", targetId, name: node?.name || "", description: "" });
  };

  const fetchMoveDirectory = async (dialog: MoveDialogState) => {
    try {
      const [directoryNodes, path] = await Promise.all([
        invoke<VfsNode[]>("vfs_list_dir"),
        invoke<Breadcrumb[]>("vfs_get_breadcrumbs").catch(() => []),
      ]);
      setMoveDialog({
        ...dialog,
        folders: directoryNodes.filter((node) => node.node_type === "Directory" && !dialog.itemIds.includes(node.id)),
        breadcrumbs: path,
      });
    } catch (reason) {
      setNotice({ title: t("Move Error"), message: String(reason) });
    }
  };

  const openMoveDialog = async (targetId: number | null) => {
    setContextMenu(null);
    const itemIds = contextIds(targetId);
    if (itemIds.length === 0) return;
    const originalPid = await invoke<number>("vfs_get_current_pid").catch(() => currentPid);
    await fetchMoveDirectory({ itemIds, originalPid, folders: [], breadcrumbs: [] });
  };

  const browseMoveFolder = async (id: number) => {
    if (!moveDialog) return;
    await invoke("vfs_enter_folder", { id });
    await fetchMoveDirectory(moveDialog);
  };

  const closeMoveDialog = async () => {
    const dialog = moveDialog;
    setMoveDialog(null);
    if (!dialog) return;
    await invoke("vfs_enter_folder", { id: dialog.originalPid }).catch(() => undefined);
  };

  const confirmMove = async () => {
    const dialog = moveDialog;
    if (!dialog) return;
    setMoveDialog(null);
    try {
      const targetPid = await invoke<number>("vfs_get_current_pid").catch(() => 0);
      setLoading(true);
      await invoke("vfs_sync_pull").catch(() => false);
      await invoke("vfs_move_items", { itemIds: dialog.itemIds, targetPid });
      await invoke("vfs_sync_push").catch(() => false);
      await invoke("vfs_enter_folder", { id: dialog.originalPid });
      setSelected(new Set());
      await fetchDirectory(false);
    } catch (reason) {
      await invoke("vfs_enter_folder", { id: dialog.originalPid }).catch(() => undefined);
      setNotice({ title: t("Move Error"), message: String(reason) });
      setLoading(false);
    }
  };

  const shareItems = async (targetId: number | null) => {
    setContextMenu(null);
    const files = contextIds(targetId).map((id) => nodeMap.get(id)).filter((node): node is VfsNode => node?.node_type === "File");
    if (files.length === 0) {
      setNotice({ title: t("Share Error"), message: t("Folders cannot be shared. Please select files.") });
      return;
    }
    try {
      const codes = await Promise.all(files.map((file) => invoke<string>("vfs_generate_share_code", { vfsId: file.id })));
      await navigator.clipboard.writeText(codes.join("\n"));
      setNotice({ title: t("Copied"), message: t("Code copied to clipboard") });
    } catch (reason) {
      setNotice({ title: t("Share Error"), message: String(reason) });
    }
  };

  const queueDownloads = async (targetId: number | null) => {
    setContextMenu(null);
    const items = contextIds(targetId).map((id) => nodeMap.get(id)).filter((node): node is VfsNode => Boolean(node));
    if (items.length === 0) return;
    try {
      let config: { useDefaultDownloadPath?: boolean; downloadPath?: string } = {};
      try { config = JSON.parse(localStorage.getItem("heriheri_config") || "{}"); } catch { /* use dialog */ }
      const picked = config.useDefaultDownloadPath && config.downloadPath
        ? config.downloadPath
        : await open({ directory: true, title: t("Select Download Folder") });
      if (!picked || Array.isArray(picked)) return;
      const directory = picked.replace(/[/\\]$/, "");
      const separator = directory.includes("\\") ? "\\" : "/";
      const active = JSON.parse(localStorage.getItem("heriheri_down_active") || "[]");

      for (const item of items) {
        if (item.node_type === "Directory") {
          const files = await invoke<Array<VfsNode & { rel_path: string }>>("vfs_get_folder_tree", { id: item.id });
          if (files.length === 0) continue;
          const groupId = `g_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`;
          active.push({ id: groupId, isGroup: true, name: item.name, status: "Queued", totalItems: files.length, finishedItems: 0 });
          files.forEach((file) => active.push({
            id: `t_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
            groupId,
            isGroup: false,
            name: file.name,
            type: "Download",
            status: "Queued",
            vfsId: file.id,
            localPath: `${directory}${separator}${file.rel_path.split("/").join(separator)}`,
            resumeOffset: 0,
            totalSize: Math.round(parseBytes(file.size)),
          }));
        } else {
          active.push({
            id: `t_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
            isGroup: false,
            name: item.name,
            type: "Download",
            status: "Queued",
            vfsId: item.id,
            localPath: `${directory}${separator}${item.name}`,
            resumeOffset: 0,
            totalSize: Math.round(parseBytes(item.size)),
          });
        }
      }
      localStorage.setItem("heriheri_down_active", JSON.stringify(active));
      window.dispatchEvent(new CustomEvent("DOWN_TASK_START"));
    } catch (reason) {
      setNotice({ title: t("Download Error"), message: String(reason) });
    }
  };

  const deleteItems = async (targetId: number | null) => {
    setContextMenu(null);
    const ids = contextIds(targetId);
    if (ids.length > 0) setDeleteDialogIds(ids);
  };

  const confirmDelete = async () => {
    const ids = deleteDialogIds;
    setDeleteDialogIds(null);
    if (!ids || ids.length === 0) return;
    try {
      setLoading(true);
      await invoke("vfs_sync_pull").catch(() => false);
      await invoke("vfs_batch_delete", { ids });
      await invoke("vfs_sync_push").catch(() => false);
      await fetchDirectory(false);
    } catch (reason) {
      setNotice({ title: t("Delete Error"), message: String(reason) });
      setLoading(false);
    }
  };

  const openNode = async (node: VfsNode) => {
    if (node.node_type === "Directory") {
      await enterFolder(node.id);
      return;
    }

    const ext = node.name.split(".").pop()?.toLowerCase() || "";
    const media = ["mp4", "mkv", "webm", "ogg", "mp3", "wav", "flac", "m4a", "aac"];
    const images = ["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg"];
    const text = ["txt", "json", "md", "csv", "py", "js", "ts", "jsx", "tsx", "c", "cpp", "h", "rs", "log", "xml"];
    const documents = ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx"];
    let route = "";
    if (media.includes(ext)) route = "player";
    else if (images.includes(ext)) route = "image";
    else if (text.includes(ext)) route = "text";
    else if (documents.includes(ext)) route = "doc";
    if (!route) return;

    let config: { webdavPort?: number } = {};
    try {
      config = JSON.parse(localStorage.getItem("heriheri_config") || "{}");
    } catch {
      // Keep the configured default when a legacy/local settings value is malformed.
    }
    const port = Number(config.webdavPort) || 8888;
    const stream = encodeURIComponent(`http://127.0.0.1:${port}/stream/${node.id}`);
    const title = encodeURIComponent(node.name);
    const isAudio = ["mp3", "wav", "flac", "m4a", "aac", "ogg"].includes(ext);
    new WebviewWindow(`concept-viewer-${node.id}`, {
      url: `index.html#/${route}?url=${stream}&title=${title}&isAudio=${isAudio}`,
      title: node.name,
      width: route === "image" ? 980 : 1100,
      height: isAudio ? 360 : 760,
      center: true,
      resizable: true,
    });
  };

  const resetSurface = () => {
    if (storageKey) localStorage.removeItem(storageKey);
    hydrateSurface(nodes, currentPid);
  };

  const lassoBox = lasso ? {
    left: Math.min(lasso.startX, lasso.currentX),
    top: Math.min(lasso.startY, lasso.currentY),
    width: Math.abs(lasso.currentX - lasso.startX),
    height: Math.abs(lasso.currentY - lasso.startY),
  } : null;
  const estimatedRoomWidth = roomRef.current?.clientWidth || Math.max(360, window.innerWidth - 420);
  const collisionColumns = Math.max(1, Math.floor((estimatedRoomWidth - 24) / (CARD_WIDTH * 1.55 + HITBOX_GAP + 12)));
  const looseItemCount = Math.max(1, nodes.length - piledIds.size);
  const pileBottom = piles.reduce((bottom, pile) => Math.max(bottom, pile.y + PILE_HEIGHT + 30), 0);
  const roomMinHeight = Math.max(410, pileBottom, SURFACE_TOP + Math.ceil(looseItemCount / collisionColumns) * (CARD_HEIGHT * 1.55 + HITBOX_GAP + 16) + 30);

  return (
    <section className="concept-desktop">
      <header className="concept-header">
        <div>
          <div className="concept-title-row">
            <h2>{t("Concept Desktop")}</h2>
            <span className="concept-experimental">{t("EXPERIMENTAL")}</span>
          </div>
          <p>{t("A spatial, pile-first view inspired by BumpTop")}</p>
        </div>
        <div className="concept-header-actions">
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder={t("Find on this surface…")} />
          <button type="button" onClick={goBack} disabled={breadcrumbs.length <= 1}>{t("Back")}</button>
          <button type="button" onClick={refresh}>{t("Refresh")}</button>
        </div>
      </header>

      <div className="concept-breadcrumbs">
        {breadcrumbs.map((crumb, index) => (
          <button type="button" key={`${crumb.id}-${index}`} onClick={() => jumpToBreadcrumb(crumb.id)}>
            {crumb.name}{index < breadcrumbs.length - 1 && <span> / </span>}
          </button>
        ))}
      </div>

      <div className="concept-toolbar" role="toolbar" aria-label={t("Concept desktop tools")}>
        <button type="button" onClick={tidy}>{t("Tidy")}</button>
        <button type="button" onClick={makePile} disabled={selected.size < 2}>{t("Make pile")}</button>
        <button type="button" onClick={scatter}>{t("Scatter")}</button>
        <button type="button" onClick={togglePinned} disabled={selected.size === 0}>{t("Pin / unpin")}</button>
        <button type="button" onClick={() => resizeSelected(0.12)} disabled={selected.size === 0}>{t("Larger")}</button>
        <button type="button" onClick={() => resizeSelected(-0.12)} disabled={selected.size === 0}>{t("Smaller")}</button>
        <span className="concept-toolbar-spacer" />
        <button type="button" className="concept-reset" onClick={() => setShowResetDialog(true)}>{t("Reset surface")}</button>
      </div>

      <div
        ref={roomRef}
        className="concept-room"
        style={{ minHeight: roomMinHeight }}
        onPointerDown={handleRoomPointerDown}
        onPointerMove={handleRoomPointerMove}
        onPointerUp={finishLasso}
        onPointerCancel={finishLasso}
        onContextMenu={(event) => openContextMenu(event, null)}
      >
        <div className="concept-wall concept-wall-left" />
        <div className="concept-wall concept-wall-right" />
        <div className="concept-wall concept-wall-back">
          <span>{t("Visual piles and positions are local to this concept view. Cloud folders are unchanged.")}</span>
        </div>
        {visibleNodes.map((node) => {
          const pose = poses[node.id] || makeDefaultPose(0, node.id, 900);
          const isDirectory = node.node_type === "Directory";
          return (
            <div
              key={node.id}
              data-concept-node-id={node.id}
              data-concept-directory={isDirectory ? "true" : "false"}
              className={`concept-item ${selected.has(node.id) ? "concept-item-selected" : ""} ${isDirectory ? "concept-item-folder" : ""} ${pose.pinned ? "concept-item-pinned" : ""} ${clipboard?.ids.includes(node.id) ? "concept-item-cut" : ""}`}
              style={{
                transform: `translate3d(${pose.x}px, ${pose.y}px, 0) rotate(${pose.rotation}deg) scale(${pose.scale})`,
                zIndex: pose.z,
              }}
              onPointerDown={(event) => handleItemPointerDown(event, node.id)}
              onPointerMove={handleItemPointerMove}
              onPointerUp={finishItemDrag}
              onPointerCancel={finishItemDrag}
              onDoubleClick={() => openNode(node)}
              onContextMenu={(event) => openContextMenu(event, node.id)}
              title={node.name}
            >
              {isDirectory && <><span className="concept-folder-sheet sheet-one" /><span className="concept-folder-sheet sheet-two" /></>}
              <div className="concept-item-face">
                <img className="concept-item-icon" src={getFileIcon(node.name, isDirectory)} alt="" draggable={false} />
                <strong>{node.name}</strong>
                <small>{isDirectory ? t("PILE / FOLDER") : node.size || node.ext?.toUpperCase() || t("FILE")}</small>
              </div>
              <span className="concept-item-edge" />
              {pose.pinned && <span className="concept-pin" aria-label={t("Pinned")}>●</span>}
            </div>
          );
        })}

        {piles.map((pile) => {
          const pileNodes = pile.nodeIds.map((id) => nodeMap.get(id)).filter((node): node is VfsNode => Boolean(node));
          const active = activePileId === pile.id;
          return (
            <div
              key={pile.id}
              className={`concept-pile concept-pile-${pile.mode} ${active ? "concept-pile-active" : ""}`}
              style={{ transform: `translate3d(${pile.x}px, ${pile.y}px, 0)` }}
              onPointerDown={(event) => handlePilePointerDown(event, pile.id)}
              onPointerMove={handlePilePointerMove}
              onPointerUp={finishPileDrag}
              onPointerCancel={finishPileDrag}
              onClick={() => setActivePileId(pile.id)}
              onWheel={(event) => {
                if (pile.mode === "fan") return;
                event.preventDefault();
                event.stopPropagation();
                const direction = event.deltaY >= 0 ? 1 : -1;
                if (pile.mode === "leaf") leafPile(pile, direction);
                else if (pile.mode === "stack") cycleStackPile(pile, direction);
                else if (pile.mode === "grid") pageGridPile(pile, direction);
              }}
            >
              <div className="concept-pile-label">
                <strong>{pile.title}</strong>
                <span>{pileNodes.length}</span>
              </div>
              <div className="concept-pile-cards">
                {pileNodes.map((node, index) => (
                  <div
                    className="concept-pile-card-slot"
                    key={node.id}
                    style={pileCardStyle(pile.mode, index, pileNodes.length, pile.leafIndex)}
                  >
                    <button
                      type="button"
                      className="concept-pile-card"
                      onClick={(event) => { event.stopPropagation(); setActivePileId(pile.id); promotePileItem(pile, node.id); }}
                      onDoubleClick={(event) => { event.stopPropagation(); openNode(node); }}
                      onContextMenu={(event) => openContextMenu(event, node.id)}
                      title={node.name}
                    >
                      <img src={getFileIcon(node.name, node.node_type === "Directory")} alt="" draggable={false} />
                      <strong>{node.name}</strong>
                    </button>
                  </div>
                ))}
              </div>
              <div className="concept-pile-mode-hint">
                {pile.mode === "fan" && t("Hover a card to magnify")}
                {pile.mode === "stack" && t("Scroll to cycle · click a card to bring it to the top")}
                {pile.mode === "grid" && t("Hover to preview · scroll to change page · double-click to open")}
                {pile.mode === "leaf" && t("Scroll to leaf through the pile")}
              </div>
              {active && (
                <div className="concept-pile-controls">
                  <button type="button" onClick={() => updatePile(pile.id, { mode: "stack" })}>{t("Stack")}</button>
                  <button type="button" onClick={() => updatePile(pile.id, { mode: "fan" })}>{t("Fan")}</button>
                  <button type="button" onClick={() => updatePile(pile.id, { mode: "grid", leafIndex: 0 })}>{t("Grid")}</button>
                  <button type="button" onClick={() => updatePile(pile.id, { mode: "leaf" })}>{t("Leaf")}</button>
                  {pile.mode === "leaf" && <>
                    <button type="button" title={t("Previous")} onClick={() => updatePile(pile.id, { leafIndex: (pile.leafIndex - 1 + pileNodes.length) % pileNodes.length })}>‹</button>
                    <button type="button" title={t("Next")} onClick={() => updatePile(pile.id, { leafIndex: (pile.leafIndex + 1) % pileNodes.length })}>›</button>
                  </>}
                  {pile.mode === "grid" && pileNodes.length > 6 && <>
                    <button type="button" title={t("Previous")} onClick={() => pageGridPile(pile, -1)}>‹</button>
                    <button type="button" title={t("Next")} onClick={() => pageGridPile(pile, 1)}>›</button>
                  </>}
                  <button type="button" className="concept-unpile" onClick={() => unpile(pile)}>{t("Unpile")}</button>
                </div>
              )}
            </div>
          );
        })}

        {lassoBox && <div className="concept-lasso" style={lassoBox} />}

        {!loading && !error && nodes.length === 0 && (
          <div className="concept-empty-state">
            <div>◇</div>
            <h3>{t("This surface is empty")}</h3>
            <p>{t("Upload files in All Files, then return here to arrange them.")}</p>
          </div>
        )}

        {error && (
          <div className="concept-empty-state concept-offline-state">
            <div>○</div>
            <h3>{t("The cloud desktop is offline")}</h3>
            <p>{t("Sign in, then retry this experimental view.")}</p>
            <button type="button" onClick={() => fetchDirectory()}>{t("Retry")}</button>
          </div>
        )}

        {loading && <div className="concept-loading"><span /><span /><span /></div>}
        <div className="concept-hint">{t("Drag empty space to lasso · drag and flick cards · double-click to open")}</div>
      </div>

      {contextMenu && (
        <div
          className="concept-context-menu"
          style={{
            left: Math.min(contextMenu.x, window.innerWidth - 180),
            top: Math.min(contextMenu.y, window.innerHeight - (contextMenu.targetId === null ? 218 : 294)),
          }}
          onPointerDown={(event) => event.stopPropagation()}
          onContextMenu={(event) => event.preventDefault()}
        >
          {contextMenu.targetId === null ? (
            <>
              <button type="button" onClick={refresh}>{t("Refresh")}</button>
              <button type="button" onClick={createFolder}>{t("New Folder")}</button>
              <button type="button" onClick={uploadFiles}>{t("Upload File")}</button>
              <button type="button" onClick={pasteItems} disabled={!clipboard}>{t("Paste")}</button>
              <button type="button" onClick={() => { setSelected(new Set(nodes.map((node) => node.id))); setContextMenu(null); }}>{t("Select All")}</button>
            </>
          ) : (
            <>
              <button type="button" onClick={() => markForMove(contextMenu.targetId)}>{t("Cut")}</button>
              <button type="button" onClick={() => openMoveDialog(contextMenu.targetId)}>{t("Move")}</button>
              <button type="button" onClick={() => shareItems(contextMenu.targetId)}>{t("Share")}</button>
              <button type="button" onClick={() => renameItem(contextMenu.targetId)} disabled={contextIds(contextMenu.targetId).length !== 1}>{t("Rename")}</button>
              <button type="button" onClick={() => queueDownloads(contextMenu.targetId)}>{t("Download")}</button>
              <div className="concept-context-divider" />
              <button type="button" className="concept-context-danger" onClick={() => deleteItems(contextMenu.targetId)}>{t("Delete")}</button>
            </>
          )}
        </div>
      )}

      {editDialog && (
        <div className="concept-modal-overlay" onPointerDown={() => setEditDialog(null)}>
          <div className="concept-modal" onPointerDown={(event) => event.stopPropagation()}>
            <h3>{editDialog.kind === "create" ? t("Create New Folder") : t("Rename Item")}</h3>
            <form onSubmit={submitEditDialog}>
              <label>{editDialog.kind === "create" ? t("Folder Name") : t("New Name")}</label>
              <input autoFocus required value={editDialog.name} onChange={(event) => setEditDialog({ ...editDialog, name: event.target.value })} />
              {editDialog.kind === "create" && <>
                <label>{t("Description")}</label>
                <input value={editDialog.description} onChange={(event) => setEditDialog({ ...editDialog, description: event.target.value })} />
              </>}
              <div className="concept-modal-actions">
                <button type="button" onClick={() => setEditDialog(null)}>{t("Cancel")}</button>
                <button type="submit" className="concept-modal-primary">{editDialog.kind === "create" ? t("Create") : t("Save")}</button>
              </div>
            </form>
          </div>
        </div>
      )}

      {moveDialog && (
        <div className="concept-modal-overlay" onPointerDown={closeMoveDialog}>
          <div className="concept-modal concept-move-modal" onPointerDown={(event) => event.stopPropagation()}>
            <h3>{t("Select Destination")}</h3>
            <div className="concept-move-breadcrumbs">
              {moveDialog.breadcrumbs.map((crumb, index) => (
                <button type="button" key={`${crumb.id}-${index}`} onClick={() => browseMoveFolder(crumb.id)}>
                  {crumb.name}{index < moveDialog.breadcrumbs.length - 1 ? " /" : ""}
                </button>
              ))}
            </div>
            <div className="concept-move-folders">
              {moveDialog.folders.length === 0 ? (
                <p>{t("No subfolders here.")}</p>
              ) : moveDialog.folders.map((folder) => (
                <button type="button" key={folder.id} onDoubleClick={() => browseMoveFolder(folder.id)}>
                  <img src={getFileIcon(folder.name, true)} alt="" draggable={false} />
                  <span>{folder.name}</span>
                  <small>{t("Double-click to enter")}</small>
                </button>
              ))}
            </div>
            <div className="concept-modal-actions">
              <button type="button" onClick={closeMoveDialog}>{t("Cancel")}</button>
              <button type="button" className="concept-modal-primary" onClick={confirmMove}>{t("Move Here")}</button>
            </div>
          </div>
        </div>
      )}

      {deleteDialogIds && (
        <div className="concept-modal-overlay" onPointerDown={() => setDeleteDialogIds(null)}>
          <div className="concept-modal" onPointerDown={(event) => event.stopPropagation()}>
            <h3>{t("Confirm Deletion")}</h3>
            <p>{deleteDialogIds.length} {t("item(s)")}{t("? This action cannot be undone.")}</p>
            <div className="concept-modal-actions">
              <button type="button" onClick={() => setDeleteDialogIds(null)}>{t("Cancel")}</button>
              <button type="button" className="concept-modal-danger" onClick={confirmDelete}>{t("Delete")}</button>
            </div>
          </div>
        </div>
      )}

      {showResetDialog && (
        <div className="concept-modal-overlay" onPointerDown={() => setShowResetDialog(false)}>
          <div className="concept-modal" onPointerDown={(event) => event.stopPropagation()}>
            <h3>{t("Reset surface")}</h3>
            <p>{t("Reset every position and visual pile on this surface? Cloud files will not be changed.")}</p>
            <div className="concept-modal-actions">
              <button type="button" onClick={() => setShowResetDialog(false)}>{t("Cancel")}</button>
              <button type="button" className="concept-modal-danger" onClick={() => { setShowResetDialog(false); resetSurface(); }}>{t("Reset")}</button>
            </div>
          </div>
        </div>
      )}

      {notice && (
        <div className="concept-modal-overlay" onPointerDown={() => setNotice(null)}>
          <div className="concept-modal" onPointerDown={(event) => event.stopPropagation()}>
            <h3>{notice.title}</h3>
            <p>{notice.message}</p>
            <div className="concept-modal-actions">
              <button type="button" className="concept-modal-primary" onClick={() => setNotice(null)}>{t("OK")}</button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
