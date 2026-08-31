import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import { GameTab, type GameTabHandle } from "./GameTab";
import i18n, { useSyncLanguage } from "./i18n";
import type { TreeDto } from "./types";
import "./App.css";

interface GameTabInfo {
  id: number;
  title: string;
}

function tabTitle(tree: TreeDto): string {
  const white = tree.tags.White && tree.tags.White !== "?" ? tree.tags.White : i18n.t("common.white");
  const black = tree.tags.Black && tree.tags.Black !== "?" ? tree.tags.Black : i18n.t("common.black");
  return `${white} - ${black}`;
}

function initialTabId(): number | null {
  const raw = new URLSearchParams(window.location.search).get("tabId");
  if (raw === null) return null;
  const id = Number(raw);
  return Number.isNaN(id) ? null : id;
}

function App() {
  useSyncLanguage();
  const { t } = useTranslation();
  const [tabs, setTabs] = useState<GameTabInfo[]>([]);
  const [activeTabId, setActiveTabId] = useState<number | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const tabRefs = useRef<Record<number, GameTabHandle | null>>({});

  // la fenetre Partie est creee cote JS et n'herite pas du menu partage
  // defini au demarrage de l'appli : on le rattache une fois montee.
  useEffect(() => {
    invoke("attach_game_menu").catch((e) => console.error(e));
  }, []);

  // premier onglet de cette fenetre : son id est passe dans l'url par l'accueil
  useEffect(() => {
    const id = initialTabId();
    if (id === null) {
      setLoadError(t("app.noTabId", { search: window.location.search }));
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
      // le dernier onglet vient d'etre ferme (on ignore l'etat initial, vide
      // le temps du chargement). destroy() plutot que close() : chaque
      // onglet est deja passe par requestClose() dans handleCloseTab, pas
      // besoin de redeclencher l'evenement close-requested (et le risque de
      // course avec sa propre resouscription, observe en pratique -- la
      // fenetre restait bloquee sur "Loading...").
      getCurrentWindow().destroy();
    }
  }, [tabs]);

  const handleTreeChange = useCallback((tabId: number, tree: TreeDto) => {
    const title = tabTitle(tree);
    setTabs((prev) => prev.map((t) => (t.id === tabId && t.title !== title ? { ...t, title } : t)));
  }, []);

  const handleCloseTab = useCallback(
    async (id: number) => {
      const proceed = await (tabRefs.current[id]?.requestClose() ?? Promise.resolve(true));
      if (!proceed) return;
      await invoke("close_tab", { tabId: id }).catch(() => {});
      delete tabRefs.current[id];
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

  // bouton X de la fenetre : verifie chaque onglet modifie (propose de
  // sauvegarder, voir GameTab.requestClose) avant de laisser la fenetre se
  // fermer reellement. destroy() (pas close()) pour le close final : evite
  // de redeclencher cet evenement indefiniment.
  useEffect(() => {
    const unlistenPromise = getCurrentWindow().onCloseRequested(async (event) => {
      event.preventDefault();
      for (const tab of tabs) {
        const proceed = await (tabRefs.current[tab.id]?.requestClose() ?? Promise.resolve(true));
        if (!proceed) return;
      }
      await getCurrentWindow().destroy();
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, [tabs]);

  if (tabs.length === 0) {
    return (
      <main className="app-layout">
        {loadError ? <p className="error">{loadError}</p> : <p className="status-message">{t("common.loading")}</p>}
      </main>
    );
  }

  return (
    <div className="app-layout">
      <div className="game-tabs-bar">
        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={"game-tab" + (tab.id === activeTabId ? " game-tab--active" : "")}
            onClick={() => setActiveTabId(tab.id)}
          >
            <span>{tab.title}</span>
            <button
              className="game-tab-close"
              title={t("app.closeTab")}
              onClick={(e) => {
                e.stopPropagation();
                handleCloseTab(tab.id);
              }}
            >
              ✕
            </button>
          </div>
        ))}
      </div>
      <div className="game-tab-content">
        {tabs.map((tab) => (
          <div key={tab.id} style={{ display: tab.id === activeTabId ? "block" : "none", width: "100%", height: "100%" }}>
            <GameTab
              ref={(el) => {
                tabRefs.current[tab.id] = el;
              }}
              tabId={tab.id}
              active={tab.id === activeTabId}
              onTreeChange={handleTreeChange}
            />
          </div>
        ))}
      </div>
    </div>
  );
}

export default App;
