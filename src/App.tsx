import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { Color, Dests, Key } from "@lichess-org/chessground/types";
import { AnnotationPanel } from "./components/AnnotationPanel";
import { Board } from "./components/Board";
import { EnginePanel } from "./components/EnginePanel";
import { Notation } from "./components/Notation";
import type { MoveDto, NodeDto, ShapeDto, TreeDto } from "./types";
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

function App() {
  const [tree, setTree] = useState<TreeDto | null>(null);
  const [currentNodeId, setCurrentNodeId] = useState<number>(0);
  const [dests, setDests] = useState<Dests>(new Map());
  const [error, setError] = useState<string | null>(null);
  const [variationChoice, setVariationChoice] = useState<NodeDto[] | null>(null);
  const [highlightedIndex, setHighlightedIndex] = useState(0);

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

  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const [engineOpen, setEngineOpen] = useState(false);

  const handleNewGame = useCallback(async () => {
    const newTree = await invoke<TreeDto>("new_game");
    setTree(newTree);
    setCurrentNodeId(newTree.root);
    setVariationChoice(null);
    setStatusMessage(null);
    setError(null);
  }, []);

  const handleImport = useCallback(async () => {
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "PGN", extensions: ["pgn"] }],
      });
      if (!path) return;
      const newTree = await invoke<TreeDto>("import_pgn", { path });
      setTree(newTree);
      setCurrentNodeId(newTree.root);
      setVariationChoice(null);
      setStatusMessage(`Partie importée depuis ${path}`);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

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
        <div className="toolbar">
          <button onClick={handleNewGame}>Nouvelle partie</button>
          <button onClick={handleImport}>Importer PGN</button>
          <button onClick={handleExport}>Exporter PGN</button>
          <button onClick={handleSave}>Sauvegarder</button>
          <button onClick={() => setEngineOpen((v) => !v)}>
            {engineOpen ? "Fermer le moteur" : "Ouvrir le moteur"}
          </button>
        </div>
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
        {engineOpen && <EnginePanel fen={currentNode.fen} />}
        <AnnotationPanel
          node={currentNode}
          onSetComment={handleSetComment}
          onToggleNag={handleToggleNag}
        />
        <h2>Notation</h2>
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
        {error && <p className="error">{error}</p>}
      </div>
    </main>
  );
}

export default App;
