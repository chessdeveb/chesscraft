import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { ask, open, save } from "@tauri-apps/plugin-dialog";
import { DockviewReact, type DockviewApi, type DockviewReadyEvent } from "dockview-react";
import { useTranslation } from "react-i18next";
import {
  EMPTY_GAMES_ADVANCED_FILTER,
  HomeContext,
  type GamesAdvancedFilter,
  type GamesSort,
  type GamesSortColumn,
  type HomeActiveSource,
  type HomeContextValue,
} from "./HomeContext";
import { homeDockviewComponents } from "./HomeDockPanels";
import { MoveGameBasePicker } from "./components/MoveGameBasePicker";
import type { BaseInfo, GameSummary, PgnGameSummary, SaveResult } from "./types";
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
  const { t } = useTranslation();
  const theme = useDockviewTheme();
  const [dockviewApi, setDockviewApi] = useState<DockviewApi | null>(null);

  // le titre d'un panneau dockview est fige au moment de addPanel() (API
  // imperative, pas reactive) : sans ceci, changer de langue ne renommerait
  // pas les onglets deja ouverts (Bases/PGN/Parties).
  useEffect(() => {
    if (!dockviewApi) return;
    dockviewApi.getPanel("bases")?.api.setTitle(t("home.panel.bases"));
    dockviewApi.getPanel("pgn")?.api.setTitle(t("home.panel.pgn"));
    dockviewApi.getPanel("parties")?.api.setTitle(t("home.panel.games"));
  }, [dockviewApi, t]);

  const [bases, setBases] = useState<BaseInfo[]>([]);
  const [recentPgns, setRecentPgns] = useState<BaseInfo[]>([]);
  const [activeSource, setActiveSource] = useState<HomeActiveSource>(null);

  const [games, setGames] = useState<GameSummary[]>([]);
  const [gamesTotal, setGamesTotal] = useState(0);
  const [gamesLoading, setGamesLoading] = useState(false);
  const [gamesPage, setGamesPage] = useState(0);
  const [gamesSearchInput, setGamesSearchInput] = useState("");
  const [gamesSearch, setGamesSearch] = useState("");
  const [gamesSort, setGamesSort] = useState<GamesSort | null>(null);
  const [gamesAdvancedOpen, setGamesAdvancedOpen] = useState(false);
  const [gamesAdvancedInput, setGamesAdvancedInput] = useState<GamesAdvancedFilter>(EMPTY_GAMES_ADVANCED_FILTER);
  const [gamesAdvancedFilter, setGamesAdvancedFilter] = useState<GamesAdvancedFilter>(EMPTY_GAMES_ADVANCED_FILTER);
  const [selectedGameIds, setSelectedGameIds] = useState<Set<number>>(new Set());

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

  // recherche/filtres avances valides explicitement (touche Entree) plutot
  // qu'a chaque frappe : sur une base de plusieurs millions de parties, une
  // recherche par sous-chaine (LIKE '%...%') ne peut pas s'appuyer sur un
  // index et scanne toute la table -- lancer ca en boucle pendant la frappe
  // rend l'interface saccadee.
  const commitGamesFilter = useCallback(() => {
    setGamesSearch(gamesSearchInput);
    setGamesAdvancedFilter(gamesAdvancedInput);
    setGamesPage(0);
  }, [gamesSearchInput, gamesAdvancedInput]);

  const activeBasePath = activeSource?.kind === "base" ? activeSource.base.path : null;
  const activeBaseMissing = activeSource?.kind === "base" ? activeSource.base.missing : false;

  // changer de base reinitialise la recherche/pagination/tri/filtres
  useEffect(() => {
    setGamesPage(0);
    setGamesSearchInput("");
    setGamesSearch("");
    setGamesSort(null);
    setGamesAdvancedOpen(false);
    setGamesAdvancedInput(EMPTY_GAMES_ADVANCED_FILTER);
    setGamesAdvancedFilter(EMPTY_GAMES_ADVANCED_FILTER);
    setSelectedGameIds(new Set());
  }, [activeBasePath]);

  const onGamesSortChange = useCallback((column: GamesSortColumn) => {
    setGamesSort((prev) => (prev?.column === column ? { column, dir: prev.dir === "asc" ? "desc" : "asc" } : { column, dir: "asc" }));
    setGamesPage(0);
  }, []);

  // recherche rapide et filtres avances sont mutuellement exclusifs cote UI :
  // basculer de l'un a l'autre vide le precedent pour eviter toute confusion
  // sur ce qui filtre reellement la liste.
  const onToggleGamesAdvanced = useCallback(() => {
    setGamesAdvancedOpen((prev) => {
      if (prev) {
        setGamesAdvancedInput(EMPTY_GAMES_ADVANCED_FILTER);
        setGamesAdvancedFilter(EMPTY_GAMES_ADVANCED_FILTER);
      } else {
        setGamesSearchInput("");
        setGamesSearch("");
      }
      setGamesPage(0);
      return !prev;
    });
  }, []);

  const onGamesAdvancedFilterChange = useCallback((patch: Partial<GamesAdvancedFilter>) => {
    setGamesAdvancedInput((prev) => ({ ...prev, ...patch }));
  }, []);

  // sur une base de plusieurs millions de parties, une recherche par
  // sous-chaine peut prendre plusieurs secondes : on suit un id de requete
  // pour ignorer une reponse perimee si une nouvelle recherche a demarre
  // entre-temps (evite d'afficher un resultat qui ne correspond plus aux
  // filtres actuels).
  const gamesRequestIdRef = useRef(0);
  useEffect(() => {
    if (!activeBasePath || activeBaseMissing) {
      setGames([]);
      setGamesTotal(0);
      setGamesLoading(false);
      return;
    }
    const requestId = ++gamesRequestIdRef.current;
    setGamesLoading(true);
    setSelectedGameIds(new Set());
    invoke<{ games: GameSummary[]; total: number }>("list_games_in_base", {
      path: activeBasePath,
      filter: gamesAdvancedOpen
        ? {
            white: gamesAdvancedFilter.white,
            black: gamesAdvancedFilter.black,
            result: gamesAdvancedFilter.result,
            dateFrom: gamesAdvancedFilter.dateFrom,
            dateTo: gamesAdvancedFilter.dateTo,
            timeControl: gamesAdvancedFilter.timeControl,
          }
        : { search: gamesSearch },
      sortBy: gamesSort?.column ?? "",
      sortDir: gamesSort?.dir ?? "",
      offset: gamesPage * GAMES_PAGE_SIZE,
      limit: GAMES_PAGE_SIZE,
    })
      .then((res) => {
        if (gamesRequestIdRef.current !== requestId) return;
        setGames(res.games);
        setGamesTotal(res.total);
      })
      .catch((e) => {
        if (gamesRequestIdRef.current !== requestId) return;
        setError(String(e));
      })
      .finally(() => {
        if (gamesRequestIdRef.current === requestId) setGamesLoading(false);
      });
  }, [activeBasePath, activeBaseMissing, gamesSearch, gamesAdvancedOpen, gamesAdvancedFilter, gamesPage, gamesSort]);

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
      const path = await save({ filters: [{ name: t("common.chessBaseFilter"), extensions: ["sqlite"] }] });
      if (!path) return;
      const info = await invoke<BaseInfo>("create_base", { name: baseNameFromPath(path), path });
      setBases((prev) => [...prev, info]);
      setActiveSource({ kind: "base", base: info });
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [t]);

  const handleAddExistingBase = useCallback(async () => {
    try {
      const path = await open({
        multiple: false,
        filters: [{ name: t("common.chessBaseFilter"), extensions: ["sqlite"] }],
      });
      if (!path) return;
      const info = await invoke<BaseInfo>("add_existing_base", { path });
      setBases((prev) => (prev.some((b) => b.path === info.path) ? prev : [...prev, info]));
      setActiveSource({ kind: "base", base: info });
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [t]);

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
        ? await ask(t("home.baseFileMissingConfirm", { name: selectedBase.name }), {
            title: t("home.baseMissingTitle"),
            kind: "warning",
          })
        : await ask(t("home.deleteBaseConfirm", { name: selectedBase.name }), {
            title: t("home.deleteBaseTitle"),
            kind: "warning",
          });
      if (!confirmed) return;
      await invoke("delete_base", { path: selectedBase.path, deleteFile: !selectedBase.missing });
      setBases((prev) => prev.filter((b) => b.path !== selectedBase.path));
      setActiveSource(null);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, [activeSource, t]);

  const handleDeleteGames = useCallback(
    async (ids: number[]) => {
      if (activeSource?.kind !== "base" || ids.length === 0) return;
      const selectedBase = activeSource.base;
      try {
        const confirmed = await ask(
          ids.length > 1 ? t("home.deleteGamesConfirm", { count: ids.length }) : t("home.deleteGameConfirm"),
          { title: t("home.deleteGameTitle"), kind: "warning" }
        );
        if (!confirmed) return;
        for (const id of ids) {
          await invoke("delete_game", { path: selectedBase.path, id });
        }
        setGames((prev) => prev.filter((g) => !ids.includes(g.id)));
        setGamesTotal((n) => Math.max(0, n - ids.length));
        setSelectedGameIds(new Set());
        refreshBases();
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [activeSource, refreshBases, t]
  );

  const handleDeleteGame = useCallback((id: number) => handleDeleteGames([id]), [handleDeleteGames]);

  // deplace/copie une a une, en sequence : le volume attendu (selection
  // manuelle par l'utilisateur) reste petit, pas besoin d'une commande cote
  // Rust dediee au traitement par lot.
  const performTransferGames = useCallback(
    async (ids: number[], fromPath: string, toPath: string, copy: boolean) => {
      try {
        for (const id of ids) {
          let force = false;
          for (;;) {
            const result = await invoke<SaveResult>("move_game", { fromPath, toPath, id, copy, force });
            if (result.kind !== "duplicate") break;
            const g = result.existing;
            const proceed = await ask(
              t("gameTab.duplicateConfirm", { white: g.white, black: g.black, date: g.date }),
              { title: t("gameTab.duplicateTitle"), kind: "warning" }
            );
            if (!proceed) break;
            force = true;
          }
        }
        if (!copy && activeSource?.kind === "base" && activeSource.base.path === fromPath) {
          setGames((prev) => prev.filter((g) => !ids.includes(g.id)));
          setGamesTotal((n) => Math.max(0, n - ids.length));
        }
        setSelectedGameIds(new Set());
        refreshBases();
        await emit("bases-changed");
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [activeSource, refreshBases, t]
  );

  const [transferPicker, setTransferPicker] = useState<{ ids: number[]; mode: "choose" | "move" | "copy" } | null>(
    null
  );

  const handleMoveGame = useCallback((id: number) => {
    setTransferPicker({ ids: [id], mode: "choose" });
  }, []);

  const handleTransferGames = useCallback((ids: number[], mode: "move" | "copy") => {
    if (ids.length === 0) return;
    setTransferPicker({ ids, mode });
  }, []);

  const handleConfirmTransferPicker = useCallback(
    (toPath: string, copy: boolean) => {
      if (activeSource?.kind !== "base" || !transferPicker) return;
      const fromPath = activeSource.base.path;
      const ids = transferPicker.ids;
      setTransferPicker(null);
      performTransferGames(ids, fromPath, toPath, copy);
    },
    [activeSource, transferPicker, performTransferGames]
  );

  const [dropConfirm, setDropConfirm] = useState<{ ids: number[]; fromPath: string; toBase: BaseInfo } | null>(null);

  const handleDropGamesOnBase = useCallback(
    (toBase: BaseInfo, ids: number[], fromPath: string, ctrlKey: boolean) => {
      if (ctrlKey) {
        performTransferGames(ids, fromPath, toBase.path, true);
      } else {
        setDropConfirm({ ids, fromPath, toBase });
      }
    },
    [performTransferGames]
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
          api.addPanel({ id: "bases", component: "bases", title: t("home.panel.bases") });
        } else if (id === "pgn") {
          const bases = api.getPanel("bases");
          api.addPanel({
            id: "pgn",
            component: "pgn",
            title: t("home.panel.pgn"),
            position: bases ? { direction: "below", referencePanel: "bases" } : undefined,
          });
        } else {
          const bases = api.getPanel("bases");
          api.addPanel({
            id: "parties",
            component: "parties",
            title: t("home.panel.games"),
            position: bases ? { direction: "right", referencePanel: "bases" } : undefined,
          });
        }
      } else {
        existing?.api.close();
      }
    },
    [dockviewApi, t]
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
    if (prefs.bases) api.addPanel({ id: "bases", component: "bases", title: t("home.panel.bases") });
    if (prefs.pgn) {
      const basesPanel = api.getPanel("bases");
      api.addPanel({
        id: "pgn",
        component: "pgn",
        title: t("home.panel.pgn"),
        position: basesPanel ? { direction: "below", referencePanel: "bases" } : undefined,
      });
    }
    if (prefs.parties) {
      const basesPanel = api.getPanel("bases");
      api.addPanel({
        id: "parties",
        component: "parties",
        title: t("home.panel.games"),
        position: basesPanel ? { direction: "right", referencePanel: "bases" } : undefined,
      });
    }
    api.getPanel("bases")?.api.setActive();
  }, [t]);

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
    gamesLoading,
    gamesPage,
    gamesSearchInput,
    onGamesSearchChange: setGamesSearchInput,
    onGamesPageChange: setGamesPage,
    gamesSort,
    onGamesSortChange,
    gamesAdvancedOpen,
    onToggleGamesAdvanced,
    gamesAdvancedFilter: gamesAdvancedInput,
    onGamesAdvancedFilterChange,
    onGamesFilterCommit: commitGamesFilter,
    onOpenBaseGame: handleOpenBaseGame,
    onDeleteGame: handleDeleteGame,
    onDeleteGames: handleDeleteGames,
    onMoveGame: handleMoveGame,
    onTransferGames: handleTransferGames,
    onDropGamesOnBase: handleDropGamesOnBase,
    selectedGameIds,
    onGamesSelectionChange: setSelectedGameIds,
    pgnGames,
    onOpenPgnGame: handleOpenPgnGame,
  };

  return (
    <HomeContext.Provider value={contextValue}>
      <div className={"dockview-container " + theme}>
        <DockviewReact components={homeDockviewComponents} onReady={onDockviewReady} />
      </div>
      {transferPicker && activeSource?.kind === "base" && (
        <MoveGameBasePicker
          excludePath={activeSource.base.path}
          mode={transferPicker.mode}
          count={transferPicker.ids.length}
          onConfirm={handleConfirmTransferPicker}
          onCancel={() => setTransferPicker(null)}
        />
      )}
      {dropConfirm && (
        <div className="modal-overlay" onClick={() => setDropConfirm(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <h2>
              {dropConfirm.ids.length > 1
                ? t("gamesTable.moveGamesTitle", { count: dropConfirm.ids.length })
                : t("gamesTable.moveGameTitle")}
            </h2>
            <p>{t("gamesTable.dropConfirmMessage", { base: dropConfirm.toBase.name })}</p>
            <div className="modal-actions">
              <button onClick={() => setDropConfirm(null)}>{t("common.cancel")}</button>
              <button
                onClick={() => {
                  performTransferGames(dropConfirm.ids, dropConfirm.fromPath, dropConfirm.toBase.path, true);
                  setDropConfirm(null);
                }}
              >
                {t("gamesTable.copyGame")}
              </button>
              <button
                onClick={() => {
                  performTransferGames(dropConfirm.ids, dropConfirm.fromPath, dropConfirm.toBase.path, false);
                  setDropConfirm(null);
                }}
              >
                {t("gamesTable.moveGame")}
              </button>
            </div>
          </div>
        </div>
      )}
    </HomeContext.Provider>
  );
}
