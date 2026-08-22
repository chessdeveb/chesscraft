import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { GameTab } from "./GameTab";
import type { TreeDto } from "./types";
import "./App.css";

interface GameTabInfo {
  id: number;
  title: string;
}

function tabTitle(tree: TreeDto): string {
  const white = tree.tags.White && tree.tags.White !== "?" ? tree.tags.White : "Blancs";
  const black = tree.tags.Black && tree.tags.Black !== "?" ? tree.tags.Black : "Noirs";
  return `${white} - ${black}`;
}

function initialTabId(): number | null {
  const raw = new URLSearchParams(window.location.search).get("tabId");
  if (raw === null) return null;
  const id = Number(raw);
  return Number.isNaN(id) ? null : id;
}

function App() {
  const [tabs, setTabs] = useState<GameTabInfo[]>([]);
  const [activeTabId, setActiveTabId] = useState<number | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  // premier onglet de cette fenetre : son id est passe dans l'url par l'accueil
  useEffect(() => {
    const id = initialTabId();
    if (id === null) {
      setLoadError(`Aucun tabId dans l'URL (${window.location.search})`);
      return;
    }
    invoke<TreeDto>("get_tree", { tabId: id })
      .then((tree) => {
        setTabs([{ id, title: tabTitle(tree) }]);
        setActiveTabId(id);
      })
      .catch((e) => setLoadError(String(e)));
  }, []);

  // l'accueil signale l'ouverture d'une partie pendant que cette fenetre est deja ouverte
  useEffect(() => {
    const unlistenPromise = listen<{ tabId: number; tree: TreeDto }>("game-tab-opened", (event) => {
      const { tabId, tree } = event.payload;
      setTabs((prev) => (prev.some((t) => t.id === tabId) ? prev : [...prev, { id: tabId, title: tabTitle(tree) }]));
      setActiveTabId(tabId);
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  const hadTabsRef = useRef(false);
  useEffect(() => {
    if (tabs.length > 0) {
      hadTabsRef.current = true;
    } else if (hadTabsRef.current) {
      // le dernier onglet vient d'etre ferme (on ignore l'etat initial, vide le temps du chargement)
      getCurrentWindow().close();
    }
  }, [tabs]);

  const handleCloseTab = useCallback(
    async (id: number) => {
      await invoke("close_tab", { tabId: id }).catch(() => {});
      setTabs((prev) => {
        const next = prev.filter((t) => t.id !== id);
        if (activeTabId === id && next.length > 0) {
          setActiveTabId(next[next.length - 1].id);
        }
        return next;
      });
    },
    [activeTabId]
  );

  if (tabs.length === 0) {
    return (
      <main className="app-layout">
        {loadError ? <p className="error">{loadError}</p> : <p className="status-message">Chargement...</p>}
      </main>
    );
  }

  return (
    <div className="app-layout">
      <div className="game-tabs-bar">
        {tabs.map((t) => (
          <div
            key={t.id}
            className={"game-tab" + (t.id === activeTabId ? " game-tab--active" : "")}
            onClick={() => setActiveTabId(t.id)}
          >
            <span>{t.title}</span>
            <button
              className="game-tab-close"
              title="Fermer cet onglet"
              onClick={(e) => {
                e.stopPropagation();
                handleCloseTab(t.id);
              }}
            >
              ✕
            </button>
          </div>
        ))}
      </div>
      <div className="game-tab-content">
        {tabs.map((t) => (
          <div key={t.id} style={{ display: t.id === activeTabId ? "block" : "none", width: "100%", height: "100%" }}>
            <GameTab tabId={t.id} active={t.id === activeTabId} />
          </div>
        ))}
      </div>
    </div>
  );
}

export default App;
