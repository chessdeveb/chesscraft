import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { ask, save } from "@tauri-apps/plugin-dialog";
import { DockviewReact, type DockviewApi, type DockviewReadyEvent } from "dockview-react";
import type { Dests, Key } from "@lichess-org/chessground/types";
import { useTranslation } from "react-i18next";
import { AppContext, type AppContextValue } from "./AppContext";
import { dockviewComponents } from "./DockPanels";
import { persistDockviewLayout, syncPanelCheckedStates, tryRestoreDockviewLayout } from "./dockviewPersistence";
import { EVAL_NAG_CODES, MOVE_NAG_CODES } from "./nag";
import { SaveBasePicker } from "./components/SaveBasePicker";
import type {
  EngineInfo,
  ExplorerGamesPage,
  GameSummary,
  MoveDto,
  NodeDto,
  OpeningMoveStat,
  SaveResult,
  ShapeDto,
  TabDto,
  TreeDto,
} from "./types";
import "@lichess-org/chessground/assets/chessground.base.css";
import "@lichess-org/chessground/assets/chessground.brown.css";
import "@lichess-org/chessground/assets/chessground.cburnett.css";
import "dockview-react/dist/styles/dockview.css";
import "./board-themes.css";
import "./App.css";

interface GameTabProps {
  tabId: number;
  active: boolean;
  isPlayerPrep?: boolean;
  onTreeChange?: (tabId: number, tree: TreeDto) => void;
}

// Expose a l'appelant (App.tsx) un point d'entree unique pour demander la
// fermeture de cet onglet : encapsule ici la decision de proposer une
// sauvegarde (l'etat "modifie" et le flux de sauvegarde vivent tous les deux
// dans GameTab). Resout a `true` si la fermeture peut continuer, `false` si
// l'utilisateur annule.
export interface GameTabHandle {
  requestClose: () => Promise<boolean>;
}

function buildDests(moves: MoveDto[]): Dests {
  const dests: Dests = new Map();
  for (const m of moves) {
    const from = m.from as Key;
    const list = dests.get(from) ?? [];
    list.push(m.to as Key);
    dests.set(from, list);
  }
  return dests;
}

// Retrouve dans l'arbre d'une partie de l'explorateur le noeud a la
// profondeur donnee en suivant la ligne principale (premier enfant a chaque
// noeud) : c'est cette meme ligne qui est comparee cote base (mainline_sans)
// pour selectionner les parties passant par la position courante, donc les
// `depth` premiers coups y correspondent forcement.
function mainlineNodeAt(tree: TreeDto, depth: number): number {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  let node = byId.get(tree.root);
  for (let i = 0; i < depth && node && node.children.length > 0; i++) {
    node = byId.get(node.children[0]);
  }
  return node ? node.id : tree.root;
}

function pathSans(tree: TreeDto, nodeId: number): string[] {
  const byId = new Map(tree.nodes.map((n) => [n.id, n]));
  const sans: string[] = [];
  let node = byId.get(nodeId);
  while (node && node.parent !== null) {
    if (node.san) sans.unshift(node.san);
    node = byId.get(node.parent);
  }
  return sans;
}

interface GamePanelPrefs {
  notation: boolean;
  explorer: boolean;
  engine: boolean;
}

const GAME_PANELS_KEY = "gamePanels";
const GAME_LAYOUT_KEY = "gameDockviewLayout";
const GAME_LAYOUT_KEY_PREP = "gameDockviewLayoutPrep";
const GAME_PANEL_IDS = ["notation", "explorer", "engine"];

function loadGamePanelPrefs(): GamePanelPrefs {
  const defaults: GamePanelPrefs = { notation: true, explorer: false, engine: false };
  try {
    const raw = localStorage.getItem(GAME_PANELS_KEY);
    if (!raw) return defaults;
    return { ...defaults, ...JSON.parse(raw) };
  } catch {
    return defaults;
  }
}

function useDockviewTheme(): string {
  const [theme, setTheme] = useState(() =>
    window.matchMedia("(prefers-color-scheme: dark)").matches ? "dockview-theme-dark" : "dockview-theme-light"
  );
  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const handler = (e: MediaQueryListEvent) => setTheme(e.matches ? "dockview-theme-dark" : "dockview-theme-light");
    mq.addEventListener("change", handler);
    return () => mq.removeEventListener("change", handler);
  }, []);
  return theme;
}

export const GameTab = forwardRef<GameTabHandle, GameTabProps>(function GameTab({ tabId, active, isPlayerPrep = false, onTreeChange }, ref) {
  const { t } = useTranslation();
  const [activeBasePath, setActiveBasePath] = useState<string | null>(null);
  const [showSaveBasePicker, setShowSaveBasePicker] = useState(false);
  const [isDirty, setIsDirty] = useState(false);
  const [closeConfirmOpen, setCloseConfirmOpen] = useState(false);
  const closeResolveRef = useRef<((proceed: boolean) => void) | null>(null);

  const [tree, setTree] = useState<TreeDto | null>(null);
  const [currentNodeId, setCurrentNodeId] = useState<number>(0);
  const [dests, setDests] = useState<Dests>(new Map());
  const [error, setError] = useState<string | null>(null);
  const [variationChoice, setVariationChoice] = useState<NodeDto[] | null>(null);
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const dockviewApiRef = useRef<DockviewApi | null>(null);
  const gameLayoutCleanupRef = useRef<(() => void) | null>(null);
  const theme = useDockviewTheme();
  const [boardTheme, setBoardThemeState] = useState(() => localStorage.getItem("boardTheme") ?? "brown");
  const setBoardTheme = useCallback((value: string) => {
    setBoardThemeState(value);
    localStorage.setItem("boardTheme", value);
  }, []);

  // hydratation initiale de cet onglet (la session existe deja cote backend :
  // creee par l'accueil via new_game/open_game/import_pgn avant l'ouverture de l'onglet)
  useEffect(() => {
    invoke<TreeDto>("get_tree", { tabId }).then((t) => {
      setTree(t);
      setCurrentNodeId(t.root);
    });
    // si la partie vient d'une base (open_game), la sauvegarde doit
    // proposer cette meme base par defaut plutot que de repartir a vide
    invoke<string | null>("get_active_base_path", { tabId }).then(setActiveBasePath);
  }, [tabId]);

  useEffect(() => {
    if (!tree) return;
    invoke<MoveDto[]>("legal_moves", { tabId, nodeId: currentNodeId })
      .then((moves) => setDests(buildDests(moves)))
      .catch(() => setDests(new Map()));
  }, [tabId, currentNodeId, tree]);

  // remonte l'arbre au parent pour qu'il puisse recalculer le titre de
  // l'onglet (ex. apres avoir renseigne les noms des joueurs a la sauvegarde)
  useEffect(() => {
    if (tree) onTreeChange?.(tabId, tree);
  }, [tree, tabId, onTreeChange]);

  const currentNode: NodeDto | undefined = tree?.nodes.find((n) => n.id === currentNodeId);

  const handleMove = useCallback(
    async (from: Key, to: Key) => {
      try {
        const newId = await invoke<number>("play_move", {
          tabId,
          nodeId: currentNodeId,
          from,
          to,
          promotion: null,
        });
        const newTree = await invoke<TreeDto>("get_tree", { tabId });
        setTree(newTree);
        setCurrentNodeId(newId);
        setVariationChoice(null);
        setError(null);
        setIsDirty(true);
      } catch (e) {
        setError(String(e));
      }
    },
    [tabId, currentNodeId]
  );

  const handleSelect = useCallback((id: number) => {
    setCurrentNodeId(id);
    setVariationChoice(null);
  }, []);

  const chooseVariation = useCallback((id: number) => {
    setCurrentNodeId(id);
    setVariationChoice(null);
  }, []);

  const refreshTree = useCallback(async () => {
    const newTree = await invoke<TreeDto>("get_tree", { tabId });
    setTree(newTree);
  }, [tabId]);

  const handleShapesChange = useCallback(
    async (shapes: ShapeDto[]) => {
      try {
        await invoke("set_shapes", { tabId, nodeId: currentNodeId, shapes });
        await refreshTree();
        setIsDirty(true);
      } catch (e) {
        setError(String(e));
      }
    },
    [tabId, currentNodeId, refreshTree]
  );

  const handleSetComment = useCallback(
    async (before: string, after: string) => {
      try {
        await invoke("set_comment", { tabId, nodeId: currentNodeId, before, after });
        await refreshTree();
        setIsDirty(true);
      } catch (e) {
        setError(String(e));
      }
    },
    [tabId, currentNodeId, refreshTree]
  );

  const handleToggleNag = useCallback(
    async (code: number) => {
      const node = tree?.nodes.find((n) => n.id === currentNodeId);
      const currentNags = node?.nags ?? [];
      const isActive = currentNags.includes(code);
      const group = MOVE_NAG_CODES.has(code) ? MOVE_NAG_CODES : EVAL_NAG_CODES;
      const withoutGroup = currentNags.filter((n) => !group.has(n));
      const nextNags = isActive ? withoutGroup : [...withoutGroup, code];
      try {
        await invoke("set_nags", { tabId, nodeId: currentNodeId, nags: nextNags });
        await refreshTree();
        setIsDirty(true);
      } catch (e) {
        setError(String(e));
      }
    },
    [tabId, currentNodeId, tree, refreshTree]
  );

  const handleExport = useCallback(async () => {
    if (!tree) return;
    try {
      const path = await save({
        filters: [{ name: "PGN", extensions: ["pgn"] }],
      });
      if (!path) return;
      await invoke("export_pgn", { tabId, path });
      setStatusMessage(t("gameTab.exportedTo", { path }));
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [tabId, tree]);

  const handleSave = useCallback(() => {
    if (!tree) return;
    setShowSaveBasePicker(true);
  }, [tree]);

  const handleConfirmSave = useCallback(
    async (path: string, force = false) => {
      try {
        const result = await invoke<SaveResult>("save_game_to", { tabId, path, force });
        if (result.kind === "duplicate") {
          const g = result.existing;
          const proceed = await ask(
            t("gameTab.duplicateConfirm", { white: g.white, black: g.black, date: g.date }),
            { title: t("gameTab.duplicateTitle"), kind: "warning" }
          );
          if (proceed) {
            await handleConfirmSave(path, true);
          }
          return;
        }
        setActiveBasePath(path);
        setShowSaveBasePicker(false);
        setStatusMessage(t("gameTab.savedTo", { path }));
        setError(null);
        setIsDirty(false);
        await refreshTree();
        await emit("bases-changed");
        if (closeResolveRef.current) {
          closeResolveRef.current(true);
          closeResolveRef.current = null;
        }
      } catch (e) {
        setError(String(e));
      }
    },
    [tabId, refreshTree]
  );

  const requestClose = useCallback((): Promise<boolean> => {
    // un onglet de preparation n'a pas vocation a etre sauvegarde : naviguer
    // dans l'explorateur y joue de vrais coups sur l'echiquier (isDirty
    // passe donc a true des le premier coup consulte), sans que ce soit une
    // "modification" au sens ou l'entend l'utilisateur pour une partie.
    if (isPlayerPrep || !isDirty) return Promise.resolve(true);
    return new Promise<boolean>((resolve) => {
      closeResolveRef.current = resolve;
      setCloseConfirmOpen(true);
    });
  }, [isPlayerPrep, isDirty]);

  useImperativeHandle(ref, () => ({ requestClose }), [requestClose]);

  // --- Moteur ---
  const [enginePath, setEnginePathState] = useState(() => localStorage.getItem("stockfishPath") ?? "");
  const [engineRunning, setEngineRunning] = useState(false);
  const [engineStarting, setEngineStarting] = useState(false);
  const [enginePaused, setEnginePaused] = useState(false);
  const [engineDepth, setEngineDepth] = useState(() => Number(localStorage.getItem("stockfishDepth")) || 18);
  const [engineMultiPv, setEngineMultiPv] = useState(() => Number(localStorage.getItem("stockfishMultiPv")) || 1);
  const [maxThreads, setMaxThreads] = useState(1);
  const [engineThreads, setEngineThreads] = useState(() => Number(localStorage.getItem("stockfishThreads")) || 1);
  const [engineLines, setEngineLines] = useState<EngineInfo[]>([]);
  const [engineError, setEngineError] = useState<string | null>(null);

  const setEnginePath = useCallback((path: string) => {
    setEnginePathState(path);
    localStorage.setItem("stockfishPath", path);
  }, []);

  useEffect(() => {
    localStorage.setItem("stockfishDepth", String(engineDepth));
  }, [engineDepth]);

  useEffect(() => {
    localStorage.setItem("stockfishMultiPv", String(engineMultiPv));
  }, [engineMultiPv]);

  useEffect(() => {
    invoke<number>("logical_cpu_count").then((n) => {
      setMaxThreads(n);
      if (!localStorage.getItem("stockfishThreads")) {
        setEngineThreads(Math.max(1, n - 1));
      }
    });
  }, []);

  useEffect(() => {
    localStorage.setItem("stockfishThreads", String(engineThreads));
  }, [engineThreads]);

  const handleStartEngine = useCallback(async () => {
    if (!enginePath) return;
    setEngineStarting(true);
    try {
      await invoke("start_engine", { tabId, path: enginePath });
      setEngineRunning(true);
      setEnginePaused(false);
      setEngineLines([]);
      setEngineError(null);
    } catch (e) {
      setEngineError(String(e));
    } finally {
      setEngineStarting(false);
    }
  }, [tabId, enginePath]);

  const handleStopEngine = useCallback(async () => {
    await invoke("stop_engine", { tabId }).catch((e) => setEngineError(String(e)));
    setEngineRunning(false);
    setEnginePaused(false);
    setEngineLines([]);
  }, [tabId]);

  const handlePauseEngine = useCallback(async () => {
    await invoke("pause_engine", { tabId }).catch((e) => setEngineError(String(e)));
    setEnginePaused(true);
  }, [tabId]);

  const handleResumeEngine = useCallback(() => {
    setEnginePaused(false);
  }, []);

  const setPanelVisible = useCallback((id: "notation" | "explorer" | "engine", visible: boolean) => {
    if (id === "explorer") setExplorerVisible(visible);
    const api = dockviewApiRef.current;
    if (!api) return;
    const existing = api.getPanel(id);
    if (visible) {
      if (existing) {
        existing.api.setActive();
        return;
      }
      if (id === "notation") {
        api.addPanel({
          id: "notation",
          component: "notation",
          title: t("gameTab.panel.notation"),
          position: { direction: "right", referencePanel: "board" },
        });
      } else if (id === "explorer") {
        const notation = api.getPanel("notation");
        api.addPanel({
          id: "explorer",
          component: "explorer",
          title: t("gameTab.panel.explorer"),
          position: notation ? { direction: "within", referencePanel: "notation" } : undefined,
        });
      } else {
        const notation = api.getPanel("notation");
        api.addPanel({
          id: "engine",
          component: "engine",
          title: t("gameTab.panel.engine"),
          position: notation ? { direction: "below", referencePanel: "notation" } : undefined,
        });
      }
    } else {
      existing?.api.close();
    }
  }, [t]);

  const toggleGamePanel = useCallback(
    (id: "notation" | "explorer" | "engine") => {
      const isVisible = !!dockviewApiRef.current?.getPanel(id);
      setPanelVisible(id, !isVisible);
    },
    [setPanelVisible]
  );

  // le moteur s'arrete quand cet onglet se ferme
  useEffect(() => {
    return () => {
      invoke("stop_engine", { tabId }).catch(() => {});
    };
  }, [tabId]);

  useEffect(() => {
    if (!engineRunning) return;
    const unlistenPromise = listen<EngineInfo>(`engine-info-${tabId}`, (event) => {
      setEngineLines((prev) => {
        const next = prev.filter((l) => l.multipv !== event.payload.multipv);
        next.push(event.payload);
        next.sort((a, b) => a.multipv - b.multipv);
        return next;
      });
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [tabId, engineRunning]);

  useEffect(() => {
    if (!engineRunning || enginePaused || !currentNode) return;
    setEngineLines([]);
    invoke("analyze_position", {
      tabId,
      fen: currentNode.fen,
      depth: engineDepth,
      multipv: engineMultiPv,
      threads: engineThreads,
    }).catch((e) => setEngineError(String(e)));
  }, [tabId, engineRunning, enginePaused, currentNode?.fen, engineDepth, engineMultiPv, engineThreads]);

  // --- Explorateur d'ouvertures ---
  const [explorerStats, setExplorerStats] = useState<OpeningMoveStat[]>([]);
  const [explorerError, setExplorerError] = useState<string | null>(null);
  const [explorerVisible, setExplorerVisible] = useState(() => isPlayerPrep || loadGamePanelPrefs().explorer);

  useEffect(() => {
    // ne pas interroger la base de reference tant que le panneau n'est pas
    // affiche : sur une base de plusieurs millions de parties, les premiers
    // coups d'une partie restent lents a agreger (voir opening_explorer cote
    // Rust), inutile de payer ce cout a chaque coup si personne ne regarde.
    if (!tree || !explorerVisible) return;
    const moves = pathSans(tree, currentNodeId);
    invoke<OpeningMoveStat[]>("opening_explorer", { tabId, moves })
      .then((stats) => {
        setExplorerStats(stats);
        setExplorerError(null);
      })
      .catch((e) => setExplorerError(String(e)));
  }, [tabId, tree, currentNodeId, explorerVisible]);

  const handleExplorerMove = useCallback(
    async (san: string) => {
      try {
        const moves = await invoke<MoveDto[]>("legal_moves", { tabId, nodeId: currentNodeId });
        const match = moves.find((m) => m.san === san);
        if (!match) return;
        await handleMove(match.from as Key, match.to as Key);
      } catch (e) {
        setError(String(e));
      }
    },
    [tabId, currentNodeId, handleMove]
  );

  // --- Explorateur : parties passant par la position courante ---
  const EXPLORER_GAMES_PAGE_SIZE = 20;
  const [explorerGames, setExplorerGames] = useState<GameSummary[]>([]);
  const [explorerGamesTotal, setExplorerGamesTotal] = useState(0);
  const [explorerGamesPage, setExplorerGamesPage] = useState(0);
  const [explorerGamesLoading, setExplorerGamesLoading] = useState(false);
  const [explorerGamesError, setExplorerGamesError] = useState<string | null>(null);
  const [explorerGamesBasePath, setExplorerGamesBasePath] = useState<string | null>(null);

  const [previewGameId, setPreviewGameId] = useState<number | null>(null);
  const [previewTree, setPreviewTree] = useState<TreeDto | null>(null);
  const [previewNodeId, setPreviewNodeId] = useState<number | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);

  const handleClosePreview = useCallback(() => {
    setPreviewGameId(null);
    setPreviewTree(null);
    setPreviewNodeId(null);
    setPreviewError(null);
  }, []);

  // changer de position dans l'arbre invalide la liste/l'aperçu precedents
  useEffect(() => {
    setExplorerGamesPage(0);
    handleClosePreview();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentNodeId]);

  useEffect(() => {
    if (!tree || !explorerVisible) return;
    const moves = pathSans(tree, currentNodeId);
    let cancelled = false;
    setExplorerGamesLoading(true);
    invoke<ExplorerGamesPage>("opening_explorer_games", {
      tabId,
      moves,
      offset: explorerGamesPage * EXPLORER_GAMES_PAGE_SIZE,
      limit: EXPLORER_GAMES_PAGE_SIZE,
    })
      .then((page) => {
        if (cancelled) return;
        setExplorerGames(page.games);
        setExplorerGamesTotal(page.total);
        setExplorerGamesBasePath(page.basePath);
        setExplorerGamesError(null);
      })
      .catch((e) => {
        if (cancelled) return;
        setExplorerGamesError(String(e));
      })
      .finally(() => {
        if (!cancelled) setExplorerGamesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [tabId, tree, currentNodeId, explorerVisible, explorerGamesPage]);

  const handleSelectPreviewGame = useCallback(
    async (id: number) => {
      // fixe l'id selectionne tout de suite (pas seulement en cas de succes) :
      // sinon, si l'invoke echoue, ni l'echiquier ni le message d'erreur ne
      // s'affichent puisque tout le bloc apercu est conditionne a un id non nul.
      setPreviewGameId(id);
      setPreviewTree(null);
      setPreviewNodeId(null);
      setPreviewError(null);
      try {
        const previewedTree = await invoke<TreeDto>("opening_explorer_preview_game", { id });
        setPreviewTree(previewedTree);
        // demarre l'apercu a la meme position que le grand echiquier plutot
        // qu'au tout debut de la partie : c'est justement cette position qui
        // a amene cette partie dans la liste.
        const depth = tree ? pathSans(tree, currentNodeId).length : 0;
        setPreviewNodeId(mainlineNodeAt(previewedTree, depth));
      } catch (e) {
        setPreviewError(String(e));
      }
    },
    [tree, currentNodeId]
  );

  const handleSelectPreviewNode = useCallback((id: number) => {
    setPreviewNodeId(id);
  }, []);

  const handlePreviewStep = useCallback(
    (direction: "start" | "prev" | "next" | "end") => {
      if (!previewTree || previewNodeId === null) return;
      const byId = new Map(previewTree.nodes.map((n) => [n.id, n]));
      if (direction === "start") {
        setPreviewNodeId(previewTree.root);
        return;
      }
      if (direction === "end") {
        let node = byId.get(previewTree.root);
        while (node && node.children.length > 0) {
          node = byId.get(node.children[0]);
        }
        if (node) setPreviewNodeId(node.id);
        return;
      }
      const node = byId.get(previewNodeId);
      if (!node) return;
      if (direction === "prev" && node.parent !== null) {
        setPreviewNodeId(node.parent);
      } else if (direction === "next" && node.children.length > 0) {
        setPreviewNodeId(node.children[0]);
      }
    },
    [previewTree, previewNodeId]
  );

  const handleOpenPreviewGame = useCallback(async () => {
    if (previewGameId === null || !explorerGamesBasePath) return;
    try {
      const tab = await invoke<TabDto>("open_game", { path: explorerGamesBasePath, id: previewGameId });
      await emit("game-tab-opened", tab);
    } catch (e) {
      setPreviewError(String(e));
    }
  }, [previewGameId, explorerGamesBasePath]);

  // --- Menus natifs : n'agissent que sur l'onglet actif ---
  useEffect(() => {
    if (!active) return;
    const unlistenPromises = [
      listen("menu-export-pgn", () => handleExport()),
      listen("menu-save-game", () => handleSave()),
      listen<string>("menu-set-board-theme", (event) => setBoardTheme(event.payload)),
      listen("menu-toggle-panel-notation", () => toggleGamePanel("notation")),
      listen("menu-toggle-panel-explorer", () => toggleGamePanel("explorer")),
      listen("menu-toggle-panel-engine", () => toggleGamePanel("engine")),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [active, handleExport, handleSave, setBoardTheme, toggleGamePanel]);

  // --- Raccourcis clavier : n'agissent que sur l'onglet actif ---
  useEffect(() => {
    if (!active) return;
    function handleKeyDown(e: KeyboardEvent) {
      if (!tree) return;

      if (variationChoice) {
        if (e.key === "Escape" || e.key === "ArrowLeft") {
          e.preventDefault();
          setVariationChoice(null);
        } else if (e.key === "ArrowDown") {
          e.preventDefault();
          setHighlightedIndex((i) => (i + 1) % variationChoice.length);
        } else if (e.key === "ArrowUp") {
          e.preventDefault();
          setHighlightedIndex((i) => (i - 1 + variationChoice.length) % variationChoice.length);
        } else if (e.key === "ArrowRight" || e.key === "Enter") {
          e.preventDefault();
          chooseVariation(variationChoice[highlightedIndex].id);
        }
        return;
      }

      const node = tree.nodes.find((n) => n.id === currentNodeId);
      if (!node) return;

      if (e.key === "ArrowLeft" && node.parent !== null) {
        e.preventDefault();
        setCurrentNodeId(node.parent);
      } else if (e.key === "ArrowRight" && node.children.length > 0) {
        e.preventDefault();
        if (node.children.length === 1) {
          setCurrentNodeId(node.children[0]);
        } else {
          const options = node.children
            .map((childId) => tree.nodes.find((n) => n.id === childId))
            .filter((n): n is NodeDto => n !== undefined);
          setHighlightedIndex(0);
          setVariationChoice(options);
        }
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [active, tree, currentNodeId, variationChoice, highlightedIndex, chooseVariation]);

  const onDockviewReady = useCallback(
    (event: DockviewReadyEvent) => {
      dockviewApiRef.current = event.api;
      const api = event.api;
      api.addPanel({ id: "board", component: "board", title: t("gameTab.panel.board") });
      if (isPlayerPrep) {
        // onglet de preparation d'adversaire : pas de coups a lister (partie
        // vide), l'explorateur (deja filtre sur ce joueur) est ce qu'on est
        // venu voir -- disposition par defaut differente d'une partie
        // normale, et memorisee separement (cle dediee) pour ne pas melanger
        // les deux.
        if (!tryRestoreDockviewLayout(api, GAME_LAYOUT_KEY_PREP)) {
          setPanelVisible("explorer", true);
          api.getPanel("explorer")?.api.setActive();
        } else {
          setExplorerVisible(!!api.getPanel("explorer"));
        }
        syncPanelCheckedStates(api, GAME_PANEL_IDS);
        gameLayoutCleanupRef.current = persistDockviewLayout(api, GAME_LAYOUT_KEY_PREP, GAME_PANEL_IDS);
        return;
      }
      if (tryRestoreDockviewLayout(api, GAME_LAYOUT_KEY)) {
        setExplorerVisible(!!api.getPanel("explorer"));
        syncPanelCheckedStates(api, GAME_PANEL_IDS);
        gameLayoutCleanupRef.current = persistDockviewLayout(api, GAME_LAYOUT_KEY, GAME_PANEL_IDS);
        return;
      }
      const prefs = loadGamePanelPrefs();
      if (prefs.notation) setPanelVisible("notation", true);
      if (prefs.explorer) setPanelVisible("explorer", true);
      if (prefs.engine) setPanelVisible("engine", true);
      api.getPanel("notation")?.api.setActive();
      syncPanelCheckedStates(api, GAME_PANEL_IDS);
      gameLayoutCleanupRef.current = persistDockviewLayout(api, GAME_LAYOUT_KEY, GAME_PANEL_IDS);
    },
    [isPlayerPrep, setPanelVisible, t]
  );

  useEffect(() => () => gameLayoutCleanupRef.current?.(), []);

  // plusieurs onglets partagent un seul menu natif : en changer d'actif doit
  // resynchroniser les cases "Affichage > panneau" sur celui-ci, sinon elles
  // restent celles du precedent onglet actif.
  useEffect(() => {
    if (!active || !dockviewApiRef.current) return;
    syncPanelCheckedStates(dockviewApiRef.current, GAME_PANEL_IDS);
  }, [active]);

  // le titre d'un panneau dockview est fige au moment de addPanel() (API
  // imperative, pas reactive) : sans ceci, changer de langue ne renommerait
  // pas les onglets deja ouverts.
  useEffect(() => {
    const api = dockviewApiRef.current;
    if (!api) return;
    api.getPanel("board")?.api.setTitle(t("gameTab.panel.board"));
    api.getPanel("notation")?.api.setTitle(t("gameTab.panel.notation"));
    api.getPanel("explorer")?.api.setTitle(t("gameTab.panel.explorer"));
    api.getPanel("engine")?.api.setTitle(t("gameTab.panel.engine"));
  }, [t]);

  const contextValue: AppContextValue = {
    tree,
    currentNode: currentNode ?? null,
    currentNodeId,
    dests,
    boardTheme,
    setBoardTheme,
    handleMove,
    handleShapesChange,
    handleSelect,
    variationChoice,
    highlightedIndex,
    chooseVariation,
    handleSetComment,
    handleToggleNag,
    explorerStats,
    explorerError,
    handleExplorerMove,
    explorerGames,
    explorerGamesTotal,
    explorerGamesPage,
    explorerGamesLoading,
    explorerGamesError,
    onExplorerGamesPageChange: setExplorerGamesPage,
    previewGameId,
    previewTree,
    previewNodeId,
    previewError,
    onSelectPreviewGame: handleSelectPreviewGame,
    onSelectPreviewNode: handleSelectPreviewNode,
    onPreviewStep: handlePreviewStep,
    onOpenPreviewGame: handleOpenPreviewGame,
    onClosePreview: handleClosePreview,
    enginePath,
    setEnginePath,
    engineRunning,
    engineStarting,
    enginePaused,
    engineDepth,
    setEngineDepth,
    engineMultiPv,
    setEngineMultiPv,
    engineThreads,
    setEngineThreads,
    maxThreads,
    engineLines,
    engineError,
    handleStartEngine,
    handleStopEngine,
    handlePauseEngine,
    handleResumeEngine,
  };

  return (
    <AppContext.Provider value={contextValue}>
      <main className="app-layout">
        {statusMessage && <p className="status-message">{statusMessage}</p>}
        {error && <p className="error">{error}</p>}
        <div className={"dockview-container " + theme}>
          <DockviewReact components={dockviewComponents} onReady={onDockviewReady} />
        </div>
      </main>
      {showSaveBasePicker && (
        <SaveBasePicker
          tabId={tabId}
          activeBasePath={activeBasePath}
          tags={tree?.tags ?? {}}
          onConfirm={handleConfirmSave}
          onCancel={() => {
            setShowSaveBasePicker(false);
            if (closeResolveRef.current) {
              closeResolveRef.current(false);
              closeResolveRef.current = null;
            }
          }}
        />
      )}
      {closeConfirmOpen && (
        <div className="modal-overlay">
          <div className="modal">
            <h2>{t("gameTab.closeConfirmTitle")}</h2>
            <p>{t("gameTab.closeConfirmMessage")}</p>
            <div className="modal-actions">
              <button
                onClick={() => {
                  setCloseConfirmOpen(false);
                  closeResolveRef.current?.(false);
                  closeResolveRef.current = null;
                }}
              >
                {t("common.cancel")}
              </button>
              <button
                onClick={() => {
                  setCloseConfirmOpen(false);
                  closeResolveRef.current?.(true);
                  closeResolveRef.current = null;
                }}
              >
                {t("gameTab.discard")}
              </button>
              <button
                onClick={() => {
                  setCloseConfirmOpen(false);
                  setShowSaveBasePicker(true);
                }}
              >
                {t("common.save")}
              </button>
            </div>
          </div>
        </div>
      )}
    </AppContext.Provider>
  );
});
