import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { Color, Dests, Key } from "@lichess-org/chessground/types";
import { AnnotationPanel } from "./components/AnnotationPanel";
import { Board } from "./components/Board";
import { EnginePanel } from "./components/EnginePanel";
import { Notation } from "./components/Notation";
import { OpeningExplorer } from "./components/OpeningExplorer";
import type {
  EngineInfo,
  MoveDto,
  NodeDto,
  OpeningMoveStat,
  PgnGameSummary,
  ShapeDto,
  TreeDto,
} from "./types";
import "@lichess-org/chessground/assets/chessground.base.css";
import "@lichess-org/chessground/assets/chessground.brown.css";
import "@lichess-org/chessground/assets/chessground.cburnett.css";
import "./App.css";

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

function App() {
  const [tree, setTree] = useState<TreeDto | null>(null);
  const [currentNodeId, setCurrentNodeId] = useState<number>(0);
  const [dests, setDests] = useState<Dests>(new Map());
  const [error, setError] = useState<string | null>(null);
  const [variationChoice, setVariationChoice] = useState<NodeDto[] | null>(null);
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);

  useEffect(() => {
    invoke<TreeDto>("get_tree").then((t) => {
      setTree(t);
      setCurrentNodeId(t.root);
    });
  }, []);

  useEffect(() => {
    if (!tree) return;
    invoke<MoveDto[]>("legal_moves", { nodeId: currentNodeId })
      .then((moves) => setDests(buildDests(moves)))
      .catch(() => setDests(new Map()));
  }, [currentNodeId, tree]);

  const currentNode: NodeDto | undefined = tree?.nodes.find((n) => n.id === currentNodeId);

  const handleMove = useCallback(
    async (from: Key, to: Key) => {
      try {
        const newId = await invoke<number>("play_move", {
          nodeId: currentNodeId,
          from,
          to,
          promotion: null,
        });
        const newTree = await invoke<TreeDto>("get_tree");
        setTree(newTree);
        setCurrentNodeId(newId);
        setVariationChoice(null);
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [currentNodeId]
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
    const newTree = await invoke<TreeDto>("get_tree");
    setTree(newTree);
  }, []);

  const handleShapesChange = useCallback(
    async (shapes: ShapeDto[]) => {
      try {
        await invoke("set_shapes", { nodeId: currentNodeId, shapes });
        await refreshTree();
      } catch (e) {
        setError(String(e));
      }
    },
    [currentNodeId, refreshTree]
  );

  const handleSetComment = useCallback(
    async (before: string, after: string) => {
      try {
        await invoke("set_comment", { nodeId: currentNodeId, before, after });
        await refreshTree();
      } catch (e) {
        setError(String(e));
      }
    },
    [currentNodeId, refreshTree]
  );

  const handleToggleNag = useCallback(
    async (code: number) => {
      const node = tree?.nodes.find((n) => n.id === currentNodeId);
      const isActive = node?.nags[0] === code;
      try {
        await invoke("set_nags", { nodeId: currentNodeId, nags: isActive ? [] : [code] });
        await refreshTree();
      } catch (e) {
        setError(String(e));
      }
    },
    [currentNodeId, tree, refreshTree]
  );

  const handleNewGame = useCallback(async () => {
    const newTree = await invoke<TreeDto>("new_game");
    setTree(newTree);
    setCurrentNodeId(newTree.root);
    setVariationChoice(null);
    setStatusMessage(null);
    setError(null);
  }, []);

  const importGameAt = useCallback(async (path: string, index: number) => {
    try {
      const newTree = await invoke<TreeDto>("import_pgn", { path, index });
      setTree(newTree);
      setCurrentNodeId(newTree.root);
      setVariationChoice(null);
      setStatusMessage(`Partie importée depuis ${path}`);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleImport = useCallback(async () => {
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "PGN", extensions: ["pgn"] }],
      });
      if (!path) return;
      const games = await invoke<PgnGameSummary[]>("list_pgn_games", { path });
      if (games.length <= 1) {
        await importGameAt(path, 0);
        return;
      }
      const picker = new WebviewWindow("game-picker", {
        url: `/?window=game-picker&path=${encodeURIComponent(path)}`,
        title: "Choisir une partie",
        width: 720,
        height: 480,
      });
      picker.once("tauri://error", (e) => setError(String(e.payload)));
    } catch (e) {
      setError(String(e));
    }
  }, [importGameAt]);

  const handleExport = useCallback(async () => {
    try {
      const path = await save({
        filters: [{ name: "PGN", extensions: ["pgn"] }],
      });
      if (!path) return;
      await invoke("export_pgn", { path });
      setStatusMessage(`Partie exportée vers ${path}`);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleSave = useCallback(async () => {
    try {
      await invoke<number>("save_game");
      setStatusMessage("Partie sauvegardée dans la base");
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  // --- Moteur ---
  const [enginePath, setEnginePathState] = useState(() => localStorage.getItem("stockfishPath") ?? "");
  const [engineOpen, setEngineOpen] = useState(false);
  const [engineRunning, setEngineRunning] = useState(false);
  const [engineStarting, setEngineStarting] = useState(false);
  const [enginePaused, setEnginePaused] = useState(false);
  const [engineDepth, setEngineDepth] = useState(() => Number(localStorage.getItem("stockfishDepth")) || 18);
  const [engineMultiPv, setEngineMultiPv] = useState(() => Number(localStorage.getItem("stockfishMultiPv")) || 1);
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

  const handleStartEngine = useCallback(async () => {
    if (!enginePath) return;
    setEngineStarting(true);
    try {
      await invoke("start_engine", { path: enginePath });
      setEngineRunning(true);
      setEnginePaused(false);
      setEngineLines([]);
      setEngineError(null);
    } catch (e) {
      setEngineError(String(e));
    } finally {
      setEngineStarting(false);
    }
  }, [enginePath]);

  const handleStopEngine = useCallback(async () => {
    await invoke("stop_engine").catch((e) => setEngineError(String(e)));
    setEngineRunning(false);
    setEnginePaused(false);
    setEngineLines([]);
  }, []);

  const handlePauseEngine = useCallback(async () => {
    await invoke("pause_engine").catch((e) => setEngineError(String(e)));
    setEnginePaused(true);
  }, []);

  const handleResumeEngine = useCallback(() => {
    setEnginePaused(false);
  }, []);

  const handleCloseEnginePanel = useCallback(() => {
    setEngineOpen(false);
    handleStopEngine();
  }, [handleStopEngine]);

  // le moteur s'arrete si la fenetre/l'appli se ferme
  useEffect(() => {
    return () => {
      invoke("stop_engine").catch(() => {});
    };
  }, []);

  // selection faite dans la fenetre separee de choix de partie
  useEffect(() => {
    const unlistenPromise = listen<{ path: string; index: number }>("import-game-selected", (event) => {
      importGameAt(event.payload.path, event.payload.index);
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [importGameAt]);

  useEffect(() => {
    if (!engineRunning) return;
    const unlistenPromise = listen<EngineInfo>("engine-info", (event) => {
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
  }, [engineRunning]);

  useEffect(() => {
    if (!engineRunning || enginePaused || !currentNode) return;
    setEngineLines([]);
    invoke("analyze_position", { fen: currentNode.fen, depth: engineDepth, multipv: engineMultiPv }).catch(
      (e) => setEngineError(String(e))
    );
  }, [engineRunning, enginePaused, currentNode?.fen, engineDepth, engineMultiPv]);

  // --- Explorateur d'ouvertures ---
  const [activeTab, setActiveTab] = useState<"notation" | "explorer">("notation");
  const [explorerStats, setExplorerStats] = useState<OpeningMoveStat[]>([]);
  const [explorerError, setExplorerError] = useState<string | null>(null);

  useEffect(() => {
    if (activeTab !== "explorer" || !tree) return;
    const moves = pathSans(tree, currentNodeId);
    invoke<OpeningMoveStat[]>("opening_explorer", { moves })
      .then((stats) => {
        setExplorerStats(stats);
        setExplorerError(null);
      })
      .catch((e) => setExplorerError(String(e)));
  }, [activeTab, tree, currentNodeId]);

  const handleExplorerMove = useCallback(
    async (san: string) => {
      try {
        const moves = await invoke<MoveDto[]>("legal_moves", { nodeId: currentNodeId });
        const match = moves.find((m) => m.san === san);
        if (!match) return;
        await handleMove(match.from as Key, match.to as Key);
      } catch (e) {
        setError(String(e));
      }
    },
    [currentNodeId, handleMove]
  );

  // --- Menus natifs ---
  useEffect(() => {
    const unlistenPromises = [
      listen("menu-new-game", () => handleNewGame()),
      listen("menu-import-pgn", () => handleImport()),
      listen("menu-export-pgn", () => handleExport()),
      listen("menu-save-game", () => handleSave()),
      listen("menu-start-engine", () => {
        setEngineOpen(true);
        if (enginePath && !engineRunning) {
          handleStartEngine();
        }
      }),
      listen("menu-stop-engine", () => handleStopEngine()),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [handleNewGame, handleImport, handleExport, handleSave, handleStartEngine, handleStopEngine, enginePath, engineRunning]);

  useEffect(() => {
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
  }, [tree, currentNodeId, variationChoice, highlightedIndex, chooseVariation]);

  if (!tree || !currentNode) {
    return <main className="app-layout">Chargement...</main>;
  }

  return (
    <main className="app-layout">
      <div className="app-column">
        {statusMessage && <p className="status-message">{statusMessage}</p>}
        <Board
          fen={currentNode.fen}
          turnColor={currentNode.turn as Color}
          dests={dests}
          shapes={currentNode.shapes}
          onMove={handleMove}
          onShapesChange={handleShapesChange}
        />
      </div>
      <div className="side-panel">
        {engineOpen && (
          <EnginePanel
            enginePath={enginePath}
            onEnginePathChange={setEnginePath}
            running={engineRunning}
            starting={engineStarting}
            paused={enginePaused}
            depth={engineDepth}
            onDepthChange={setEngineDepth}
            multiPv={engineMultiPv}
            onMultiPvChange={setEngineMultiPv}
            lines={engineLines}
            error={engineError}
            onStart={handleStartEngine}
            onStop={handleStopEngine}
            onPause={handlePauseEngine}
            onResume={handleResumeEngine}
            onClose={handleCloseEnginePanel}
          />
        )}
        <AnnotationPanel
          node={currentNode}
          onSetComment={handleSetComment}
          onToggleNag={handleToggleNag}
        />
        <div className="tabs">
          <button
            className={"tab" + (activeTab === "notation" ? " tab--active" : "")}
            onClick={() => setActiveTab("notation")}
          >
            Notation
          </button>
          <button
            className={"tab" + (activeTab === "explorer" ? " tab--active" : "")}
            onClick={() => setActiveTab("explorer")}
          >
            Explorateur
          </button>
        </div>
        {activeTab === "notation" ? (
          <>
            {variationChoice && (
              <div className="variation-choice">
                <p>Continuer avec :</p>
                <div className="variation-choice-options">
                  {variationChoice.map((option, index) => (
                    <button
                      key={option.id}
                      className={index === highlightedIndex ? "variation-choice-highlighted" : ""}
                      onClick={() => chooseVariation(option.id)}
                    >
                      {option.san}
                    </button>
                  ))}
                </div>
              </div>
            )}
            <Notation tree={tree} currentNodeId={currentNodeId} onSelect={handleSelect} />
          </>
        ) : (
          <OpeningExplorer stats={explorerStats} error={explorerError} onSelectMove={handleExplorerMove} />
        )}
        {error && <p className="error">{error}</p>}
      </div>
    </main>
  );
}

export default App;
