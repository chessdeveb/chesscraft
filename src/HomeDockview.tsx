import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { DockviewReact, type DockviewApi, type DockviewReadyEvent } from "dockview-react";
import { HomeContext, type HomeActiveSource, type HomeContextValue } from "./HomeContext";
import { homeDockviewComponents } from "./HomeDockPanels";
import type { BaseInfo, GameSummary, PgnGameSummary } from "./types";
import "dockview-react/dist/styles/dockview.css";
import "./App.css";

interface HomeDockviewProps {
  focusBasePath: string | null;
  onOpenGame: (path: string, id: number) => void;
  onFocusConsumed: () => void;
  onOpenPgnGame: (path: string, index: number) => void;
  onImportNewFile: () => Promise<void>;
  basesVersion: number;
}

interface HomePanelPrefs {
  bases: boolean;
  pgn: boolean;
  parties: boolean;
}

const HOME_PANELS_KEY = "homePanels";

function loadHomePanelPrefs(): HomePanelPrefs {
  const defaults: HomePanelPrefs = { bases: true, pgn: true, parties: true };
  try {
    const raw = localStorage.getItem(HOME_PANELS_KEY);
    if (!raw) return defaults;
    return { ...defaults, ...JSON.parse(raw) };
  } catch {
    return defaults;
  }
}

function saveHomePanelPrefs(prefs: HomePanelPrefs) {
  localStorage.setItem(HOME_PANELS_KEY, JSON.stringify(prefs));
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

function baseNameFromPath(path: string): string {
  const fileName = path.split(/[\\/]/).pop() ?? path;
  return fileName.replace(/\.sqlite$/i, "");
}

const GAMES_PAGE_SIZE = 200;

export function HomeDockview({
  focusBasePath,
  onOpenGame,
  onFocusConsumed,
  onOpenPgnGame,
  onImportNewFile,
  basesVersion,
}: HomeDockviewProps) {
  const theme = useDockviewTheme();
  const [dockviewApi, setDockviewApi] = useState<DockviewApi | null>(null);

  const [bases, setBases] = useState<BaseInfo[]>([]);
  const [recentPgns, setRecentPgns] = useState<BaseInfo[]>([]);
  const [activeSource, setActiveSource] = useState<HomeActiveSource>(null);

  const [games, setGames] = useState<GameSummary[]>([]);
  const [gamesTotal, setGamesTotal] = useState(0);
  const [gamesPage, setGamesPage] = useState(0);
  const [gamesSearchInput, setGamesSearchInput] = useState("");
  const [gamesSearch, setGamesSearch] = useState("");

  const [pgnGames, setPgnGames] = useState<PgnGameSummary[]>([]);

  const [error, setError] = useState<string | null>(null);

  const refreshBases = useCallback(async () => {
    try {
      const list = await invoke<BaseInfo[]>("list_bases");
      setBases(list);
      setActiveSource((prev) => {
        if (prev?.kind !== "base") return prev;
        const match = list.find((b) => b.path === prev.base.path);
        return match ? { kind: "base", base: match } : prev;
      });
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const refreshRecentPgns = useCallback(async () => {
    try {
      const list = await invoke<BaseInfo[]>("list_recent_pgns");
      setRecentPgns(list);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    refreshBases();
    refreshRecentPgns();
  }, [refreshBases, refreshRecentPgns, basesVersion]);

  useEffect(() => {
    if (!focusBasePath) return;
    const match = bases.find((b) => b.path === focusBasePath);
    if (match) {
      setActiveSource({ kind: "base", base: match });
      onFocusConsumed();
    }
  }, [focusBasePath, bases, onFocusConsumed]);

  // recherche debouncee : on ne relance pas une requete a chaque frappe
  useEffect(() => {
    const t = setTimeout(() => {
      setGamesSearch(gamesSearchInput);
      setGamesPage(0);
    }, 300);
    return () => clearTimeout(t);
  }, [gamesSearchInput]);

  const activeBasePath = activeSource?.kind === "base" ? activeSource.base.path : null;
  const activeBaseMissing = activeSource?.kind === "base" ? activeSource.base.missing : false;

  // changer de base reinitialise la recherche/pagination
  useEffect(() => {
    setGamesPage(0);
    setGamesSearchInput("");
    setGamesSearch("");
  }, [activeBasePath]);

  useEffect(() => {
    if (!activeBasePath || activeBaseMissing) {
      setGames([]);
      setGamesTotal(0);
      return;
    }
    invoke<{ games: GameSummary[]; total: number }>("list_games_in_base", {
      path: activeBasePath,
      search: gamesSearch,
      offset: gamesPage * GAMES_PAGE_SIZE,
      limit: GAMES_PAGE_SIZE,
    })
      .then((res) => {
        setGames(res.games);
        setGamesTotal(res.total);
      })
      .catch((e) => setError(String(e)));
  }, [activeBasePath, activeBaseMissing, gamesSearch, gamesPage]);

  const activePgnPath = activeSource?.kind === "pgn" ? activeSource.pgn.path : null;

  useEffect(() => {
    if (!activePgnPath) {
      setPgnGames([]);
      return;
    }
    invoke<PgnGameSummary[]>("list_pgn_games", { path: activePgnPath })
      .then(setPgnGames)
      .catch((e) => setError(String(e)));
  }, [activePgnPath]);

  const handleCreateBase = useCallback(async () => {
    try {
      const path = await save({ filters: [{ name: "Base d'échecs", extensions: ["sqlite"] }] });
      if (!path) return;
      const info = await invoke<BaseInfo>("create_base", { name: baseNameFromPath(path), path });
      setBases((prev) => [...prev, info]);
      setActiveSource({ kind: "base", base: info });
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleAddExistingBase = useCallback(async () => {
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: "Base d'échecs", extensions: ["sqlite"] }],
      });
      if (!path) return;
      const info = await invoke<BaseInfo>("add_existing_base", { path });
      setBases((prev) => (prev.some((b) => b.path === info.path) ? prev : [...prev, info]));
      setActiveSource({ kind: "base", base: info });
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleImportClick = useCallback(async () => {
    await onImportNewFile();
    refreshRecentPgns();
  }, [onImportNewFile, refreshRecentPgns]);

  const handleToggleReference = useCallback(async () => {
    if (activeSource?.kind !== "base") return;
    const selectedBase = activeSource.base;
    try {
      const nextValue = !selectedBase.isReference;
      await invoke("set_base_reference", { path: selectedBase.path, isReference: nextValue });
      const updated = { ...selectedBase, isReference: nextValue };
      setActiveSource({ kind: "base", base: updated });
      // une seule base de reference a la fois : le backend a deja retire le
      // marquage des autres, on reflete ca cote frontend sans tout refetcher
      setBases((prev) =>
        prev.map((b) => (b.path === updated.path ? updated : { ...b, isReference: nextValue ? false : b.isReference }))
      );
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [activeSource]);

  const handleDeleteBase = useCallback(async () => {
    if (activeSource?.kind !== "base") return;
    const selectedBase = activeSource.base;
    try {
      const confirmed = selectedBase.missing
        ? await ask(`Le fichier de "${selectedBase.name}" est introuvable. Retirer cette base de la liste ?`, {
            title: "Base introuvable",
            kind: "warning",
          })
        : await ask(
            `Supprimer définitivement la base "${selectedBase.name}" et son fichier ? Cette action est irréversible.`,
            { title: "Supprimer la base", kind: "warning" }
          );
      if (!confirmed) return;
      await invoke("delete_base", { path: selectedBase.path, deleteFile: !selectedBase.missing });
      setBases((prev) => prev.filter((b) => b.path !== selectedBase.path));
      setActiveSource(null);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [activeSource]);

  const handleDeleteGame = useCallback(
    async (id: number) => {
      if (activeSource?.kind !== "base") return;
      const selectedBase = activeSource.base;
      try {
        const confirmed = await ask("Supprimer définitivement cette partie ? Cette action est irréversible.", {
          title: "Supprimer la partie",
          kind: "warning",
        });
        if (!confirmed) return;
        await invoke("delete_game", { path: selectedBase.path, id });
        setGames((prev) => prev.filter((g) => g.id !== id));
        setGamesTotal((t) => Math.max(0, t - 1));
        refreshBases();
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [activeSource, refreshBases]
  );

  const handleOpenBaseGame = useCallback(
    (id: number) => {
      if (activeSource?.kind !== "base") return;
      onOpenGame(activeSource.base.path, id);
    },
    [activeSource, onOpenGame]
  );

  const handleOpenPgnGame = useCallback(
    (index: number) => {
      if (activeSource?.kind !== "pgn") return;
      onOpenPgnGame(activeSource.pgn.path, index);
    },
    [activeSource, onOpenPgnGame]
  );

  // --- panneaux : visibilite persistee, pilotee par le menu natif Affichage ---
  const setPanelVisible = useCallback(
    (id: "bases" | "pgn" | "parties", visible: boolean) => {
      const api = dockviewApi;
      if (!api) return;
      const existing = api.getPanel(id);
      if (visible) {
        if (existing) {
          existing.api.setActive();
          return;
        }
        if (id === "bases") {
          api.addPanel({ id: "bases", component: "bases", title: "Bases" });
        } else if (id === "pgn") {
          const bases = api.getPanel("bases");
          api.addPanel({
            id: "pgn",
            component: "pgn",
            title: "PGN",
            position: bases ? { direction: "below", referencePanel: "bases" } : undefined,
          });
        } else {
          const bases = api.getPanel("bases");
          api.addPanel({
            id: "parties",
            component: "parties",
            title: "Parties",
            position: bases ? { direction: "right", referencePanel: "bases" } : undefined,
          });
        }
      } else {
        existing?.api.close();
      }
    },
    [dockviewApi]
  );

  useEffect(() => {
    const unlistenPromises = (["bases", "pgn", "parties"] as const).map((id) =>
      listen(`menu-toggle-panel-${id}`, () => {
        const prefs = loadHomePanelPrefs();
        const next = { ...prefs, [id]: !prefs[id] };
        saveHomePanelPrefs(next);
        setPanelVisible(id, next[id]);
      })
    );
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [setPanelVisible]);

  const onDockviewReady = useCallback((event: DockviewReadyEvent) => {
    setDockviewApi(event.api);
    const api = event.api;
    const prefs = loadHomePanelPrefs();
    if (prefs.bases) api.addPanel({ id: "bases", component: "bases", title: "Bases" });
    if (prefs.pgn) {
      const basesPanel = api.getPanel("bases");
      api.addPanel({
        id: "pgn",
        component: "pgn",
        title: "PGN",
        position: basesPanel ? { direction: "below", referencePanel: "bases" } : undefined,
      });
    }
    if (prefs.parties) {
      const basesPanel = api.getPanel("bases");
      api.addPanel({
        id: "parties",
        component: "parties",
        title: "Parties",
        position: basesPanel ? { direction: "right", referencePanel: "bases" } : undefined,
      });
    }
    api.getPanel("bases")?.api.setActive();
  }, []);

  const contextValue: HomeContextValue = {
    error,
    bases,
    onSelectBase: (base) => setActiveSource({ kind: "base", base }),
    onCreateBase: handleCreateBase,
    onAddExistingBase: handleAddExistingBase,
    recentPgns,
    onSelectPgn: (pgn) => setActiveSource({ kind: "pgn", pgn }),
    onImportClick: handleImportClick,
    activeSource,
    onToggleReference: handleToggleReference,
    onDeleteBase: handleDeleteBase,
    games,
    gamesTotal,
    gamesPage,
    gamesSearchInput,
    onGamesSearchChange: setGamesSearchInput,
    onGamesPageChange: setGamesPage,
    onOpenBaseGame: handleOpenBaseGame,
    onDeleteGame: handleDeleteGame,
    pgnGames,
    onOpenPgnGame: handleOpenPgnGame,
  };

  return (
    <HomeContext.Provider value={contextValue}>
      <div className={"dockview-container " + theme}>
        <DockviewReact components={homeDockviewComponents} onReady={onDockviewReady} />
      </div>
    </HomeContext.Provider>
  );
}
