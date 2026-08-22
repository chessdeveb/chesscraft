import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { ask, save } from "@tauri-apps/plugin-dialog";
import { DockviewReact, type DockviewApi, type DockviewReadyEvent } from "dockview-react";
import type { Dests, Key } from "@lichess-org/chessground/types";
import { AppContext, type AppContextValue } from "./AppContext";
import { dockviewComponents } from "./DockPanels";
import { EVAL_NAG_CODES, MOVE_NAG_CODES } from "./nag";
import { SaveBasePicker } from "./components/SaveBasePicker";
import type { EngineInfo, MoveDto, NodeDto, OpeningMoveStat, SaveResult, ShapeDto, TreeDto } from "./types";
import "@lichess-org/chessground/assets/chessground.base.css";
import "@lichess-org/chessground/assets/chessground.brown.css";
import "@lichess-org/chessground/assets/chessground.cburnett.css";
import "dockview-react/dist/styles/dockview.css";
import "./board-themes.css";
import "./App.css";

interface GameTabProps {
  tabId: number;
  active: boolean;
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

function loadGamePanelPrefs(): GamePanelPrefs {
  const defaults: GamePanelPrefs = { notation: true, explorer: true, engine: false };
  try {
    const raw = localStorage.getItem(GAME_PANELS_KEY);
    if (!raw) return defaults;
    return { ...defaults, ...JSON.parse(raw) };
  } catch {
    return defaults;
  }
}

function saveGamePanelPrefs(prefs: GamePanelPrefs) {
  localStorage.setItem(GAME_PANELS_KEY, JSON.stringify(prefs));
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

export function GameTab({ tabId, active }: GameTabProps) {
  const [activeBasePath, setActiveBasePath] = useState<string | null>(null);
  const [showSaveBasePicker, setShowSaveBasePicker] = useState(false);

  const [tree, setTree] = useState<TreeDto | null>(null);
  const [currentNodeId, setCurrentNodeId] = useState<number>(0);
  const [dests, setDests] = useState<Dests>(new Map());
  const [error, setError] = useState<string | null>(null);
  const [variationChoice, setVariationChoice] = useState<NodeDto[] | null>(null);
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const dockviewApiRef = useRef<DockviewApi | null>(null);
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
  }, [tabId]);

  useEffect(() => {
    if (!tree) return;
    invoke<MoveDto[]>("legal_moves", { tabId, nodeId: currentNodeId })
      .then((moves) => setDests(buildDests(moves)))
      .catch(() => setDests(new Map()));
  }, [tabId, currentNodeId, tree]);

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
      setStatusMessage(`Partie exportée vers ${path}`);
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
            `Cette partie existe déjà dans cette base (${g.white} - ${g.black}, ${g.date}). L'ajouter quand même ?`,
            { title: "Partie déjà présente", kind: "warning" }
          );
          if (proceed) {
            await handleConfirmSave(path, true);
          }
          return;
        }
        setActiveBasePath(path);
        setShowSaveBasePicker(false);
        setStatusMessage(`Partie sauvegardée dans ${path}`);
        setError(null);
        await refreshTree();
        await emit("bases-changed");
      } catch (e) {
        setError(String(e));
      }
    },
    [tabId, refreshTree]
  );

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
          title: "Notation",
          position: { direction: "right", referencePanel: "board" },
        });
      } else if (id === "explorer") {
        const notation = api.getPanel("notation");
        api.addPanel({
          id: "explorer",
          component: "explorer",
          title: "Explorateur",
          position: notation ? { direction: "within", referencePanel: "notation" } : undefined,
        });
      } else {
        const notation = api.getPanel("notation");
        api.addPanel({
          id: "engine",
          component: "engine",
          title: "Moteur",
          position: notation ? { direction: "below", referencePanel: "notation" } : undefined,
        });
      }
    } else {
      existing?.api.close();
    }
  }, []);

  const toggleGamePanel = useCallback(
    (id: "notation" | "explorer" | "engine") => {
      const prefs = loadGamePanelPrefs();
      const next = { ...prefs, [id]: !prefs[id] };
      saveGamePanelPrefs(next);
      setPanelVisible(id, next[id]);
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

  useEffect(() => {
    if (!tree) return;
    const moves = pathSans(tree, currentNodeId);
    invoke<OpeningMoveStat[]>("opening_explorer", { moves })
      .then((stats) => {
        setExplorerStats(stats);
        setExplorerError(null);
      })
      .catch((e) => setExplorerError(String(e)));
  }, [tabId, tree, currentNodeId]);

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
      api.addPanel({ id: "board", component: "board", title: "Échiquier" });
      const prefs = loadGamePanelPrefs();
      if (prefs.notation) setPanelVisible("notation", true);
      if (prefs.explorer) setPanelVisible("explorer", true);
      if (prefs.engine) setPanelVisible("engine", true);
      api.getPanel("notation")?.api.setActive();
    },
    [setPanelVisible]
  );

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
          activeBasePath={activeBasePath}
          tags={tree?.tags ?? {}}
          onConfirm={handleConfirmSave}
          onCancel={() => setShowSaveBasePicker(false)}
        />
      )}
    </AppContext.Provider>
  );
}
