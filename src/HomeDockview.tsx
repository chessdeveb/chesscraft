import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { ask, message, open, save } from "@tauri-apps/plugin-dialog";
import { DockviewReact, type DockviewApi, type DockviewReadyEvent } from "dockview-react";
import { useTranslation } from "react-i18next";
import {
  EMPTY_FIDE_ADVANCED_FILTER,
  EMPTY_GAMES_ADVANCED_FILTER,
  HomeContext,
  type FideAdvancedFilter,
  type FideSort,
  type FideSortColumn,
  type GamesAdvancedFilter,
  type GamesSort,
  type GamesSortColumn,
  type HomeActiveSource,
  type HomeContextValue,
  type PlayersSort,
  type PlayersSortColumn,
  type TournamentsSort,
  type TournamentsSortColumn,
} from "./HomeContext";
import { persistDockviewLayout, syncPanelCheckedStates, tryRestoreDockviewLayout, useDockviewTheme } from "./dockviewPersistence";
import { homeDockviewComponents } from "./HomeDockPanels";
import { ExternalUpdatesReviewDialog } from "./components/ExternalUpdatesReviewDialog";
import { MoveGameBasePicker } from "./components/MoveGameBasePicker";
import { useAutoDismissError } from "./useAutoDismissError";
import type {
  BaseInfo,
  CatchupRange,
  FidePlayer,
  FidePlayersPage,
  FideStatus,
  GameSummary,
  PgnGameSummary,
  PlayerSummary,
  PlayersPage,
  SaveResult,
  TournamentSummary,
  TournamentsPage,
  UpdatePreview,
} from "./types";
import "dockview-react/dist/styles/dockview.css";
import "./App.css";

interface HomeDockviewProps {
  onOpenGame: (path: string, id: number) => void;
  onOpenPgnGame: (path: string, index: number) => void;
  onImportNewFile: () => Promise<void>;
  basesVersion: number;
  onPrepareAgainstPlayer: (name: string, white: boolean) => void;
  onImportExternalUpdates: (dbPath: string, pgnPath: string) => void;
}

interface HomePanelPrefs {
  bases: boolean;
  pgn: boolean;
  parties: boolean;
  players: boolean;
  fide: boolean;
  tournaments: boolean;
}

const HOME_PANELS_KEY = "homePanels";
const HOME_LAYOUT_KEY = "homeDockviewLayout";
const HOME_PANEL_IDS = ["bases", "pgn", "parties", "players", "fide", "tournaments"];

function loadHomePanelPrefs(): HomePanelPrefs {
  const defaults: HomePanelPrefs = { bases: true, pgn: true, parties: true, players: true, fide: true, tournaments: true };
  try {
    const raw = localStorage.getItem(HOME_PANELS_KEY);
    if (!raw) return defaults;
    return { ...defaults, ...JSON.parse(raw) };
  } catch {
    return defaults;
  }
}

const EXTERNAL_UPDATES_PREVIEW_KEY = "externalUpdatesPreview";

// Voir le commentaire au point d'utilisation (externalUpdatesPreview) pour la
// raison d'etre de cette persistance -- try/catch comme loadHomePanelPrefs :
// un JSON perime (ex. ancien schema UpdatePreview apres une mise a jour de
// l'appli) ne doit jamais faire planter l'ouverture de l'accueil, juste
// repartir sans revue a restaurer.
function loadPersistedExternalUpdatesPreview(): UpdatePreview | null {
  try {
    const raw = localStorage.getItem(EXTERNAL_UPDATES_PREVIEW_KEY);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}

function baseNameFromPath(path: string): string {
  const fileName = path.split(/[\\/]/).pop() ?? path;
  return fileName.replace(/\.sqlite$/i, "");
}

const GAMES_PAGE_SIZE = 200;
const PLAYERS_PAGE_SIZE = 200;
const FIDE_PAGE_SIZE = 200;
const TOURNAMENTS_PAGE_SIZE = 200;

// La FIDE publie une nouvelle liste de classement le 1er de chaque mois :
// last_updated (epoch secondes, voir get_fide_status) date donc du mois en
// cours des qu'un refresh a ete fait depuis sa publication, quel que soit le
// nombre de jours ecoules -- une comparaison "il y a plus de N jours" se
// declencherait trop tot ou trop tard selon la position dans le mois.
function isCurrentMonthFideUpdate(epochSecondsStr: string | null): boolean {
  if (!epochSecondsStr) return false;
  const seconds = Number(epochSecondsStr);
  if (Number.isNaN(seconds)) return false;
  const updated = new Date(seconds * 1000);
  const now = new Date();
  return updated.getFullYear() === now.getFullYear() && updated.getMonth() === now.getMonth();
}

// Au-dela de ce retard (~4 mois, choix explicite de l'utilisateur -- voir
// LICHESS_LOOKBACK_WEEKS cote Rust, meme valeur), on demande confirmation
// avant de lancer le rattrapage complet (potentiellement plusieurs dizaines
// d'editions TWIC + recoupement Lichess par tournoi).
const LARGE_CATCHUP_WARNING_WEEKS = 17;

export function HomeDockview({
  onOpenGame,
  onOpenPgnGame,
  onImportNewFile,
  basesVersion,
  onPrepareAgainstPlayer,
  onImportExternalUpdates,
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
    dockviewApi.getPanel("players")?.api.setTitle(t("home.panel.players"));
    dockviewApi.getPanel("fide")?.api.setTitle(t("home.panel.fide"));
    dockviewApi.getPanel("tournaments")?.api.setTitle(t("home.panel.tournaments"));
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

  const [players, setPlayers] = useState<PlayerSummary[]>([]);
  const [playersTotal, setPlayersTotal] = useState(0);
  const [playersLoading, setPlayersLoading] = useState(false);
  const [playersPage, setPlayersPage] = useState(0);
  const [playersSearchInput, setPlayersSearchInput] = useState("");
  const [playersSearch, setPlayersSearch] = useState("");
  const [playersSort, setPlayersSort] = useState<PlayersSort | null>(null);

  const onPlayersSortChange = useCallback((column: PlayersSortColumn) => {
    setPlayersSort((prev) => (prev?.column === column ? { column, dir: prev.dir === "asc" ? "desc" : "asc" } : { column, dir: "asc" }));
    setPlayersPage(0);
  }, []);

  const [tournaments, setTournaments] = useState<TournamentSummary[]>([]);
  const [tournamentsTotal, setTournamentsTotal] = useState(0);
  const [tournamentsLoading, setTournamentsLoading] = useState(false);
  const [tournamentsPage, setTournamentsPage] = useState(0);
  const [tournamentsSearchInput, setTournamentsSearchInput] = useState("");
  const [tournamentsSearch, setTournamentsSearch] = useState("");
  const [tournamentsSort, setTournamentsSort] = useState<TournamentsSort | null>(null);
  const [tournamentsVersion, setTournamentsVersion] = useState(0);

  const onTournamentsSortChange = useCallback((column: TournamentsSortColumn) => {
    setTournamentsSort((prev) => (prev?.column === column ? { column, dir: prev.dir === "asc" ? "desc" : "asc" } : { column, dir: "asc" }));
    setTournamentsPage(0);
  }, []);

  // Base FIDE (reference globale, independante de la base de parties
  // selectionnee -- voir fide.rs) : pas de activeBasePath dans les deps de
  // son effet de chargement, contrairement a `players` ci-dessus.
  const [fidePlayers, setFidePlayers] = useState<FidePlayer[]>([]);
  const [fideTotal, setFideTotal] = useState(0);
  const [fideLoading, setFideLoading] = useState(false);
  const [fidePage, setFidePage] = useState(0);
  const [fideSearchInput, setFideSearchInput] = useState("");
  const [fideSearch, setFideSearch] = useState("");
  const [fideAdvancedOpen, setFideAdvancedOpen] = useState(false);
  const [fideAdvancedInput, setFideAdvancedInput] = useState<FideAdvancedFilter>(EMPTY_FIDE_ADVANCED_FILTER);
  const [fideAdvancedFilter, setFideAdvancedFilter] = useState<FideAdvancedFilter>(EMPTY_FIDE_ADVANCED_FILTER);
  const [fideSort, setFideSort] = useState<FideSort | null>(null);
  const [fideLastUpdated, setFideLastUpdated] = useState<string | null>(null);
  const [fideRefreshing, setFideRefreshing] = useState(false);
  const [fideRefreshProgress, setFideRefreshProgress] = useState(0);
  const [fideRefreshError, setFideRefreshError] = useAutoDismissError();

  const onFideSortChange = useCallback((column: FideSortColumn) => {
    setFideSort((prev) => (prev?.column === column ? { column, dir: prev.dir === "asc" ? "desc" : "asc" } : { column, dir: "asc" }));
    setFidePage(0);
  }, []);

  const commitFideFilter = useCallback(() => {
    setFideSearch(fideSearchInput);
    setFideAdvancedFilter(fideAdvancedInput);
    setFidePage(0);
  }, [fideSearchInput, fideAdvancedInput]);

  // meme principe que onToggleGamesAdvanced : recherche rapide par nom et
  // filtres avances (pays/titre/sexe/annee de naissance) sont mutuellement
  // exclusifs cote UI, basculer de l'un a l'autre vide le precedent.
  const onToggleFideAdvanced = useCallback(() => {
    setFideAdvancedOpen((prev) => {
      if (prev) {
        setFideAdvancedInput(EMPTY_FIDE_ADVANCED_FILTER);
        setFideAdvancedFilter(EMPTY_FIDE_ADVANCED_FILTER);
      } else {
        setFideSearchInput("");
        setFideSearch("");
      }
      setFidePage(0);
      return !prev;
    });
  }, []);

  const onFideAdvancedFilterChange = useCallback((patch: Partial<FideAdvancedFilter>) => {
    setFideAdvancedInput((prev) => ({ ...prev, ...patch }));
  }, []);

  const [error, setError] = useAutoDismissError();

  // Mises a jour externes (TWIC/Lichess, voir external_updates.rs) : toujours
  // operation sur la base actuellement marquee "reference" (pas de
  // base-picker dedie), pour que l'utilisateur alternant entre plusieurs
  // bases de reference obtienne toujours un rattrapage calcule depuis la
  // partie la plus recente de celle en cours (voir infer_start_issue, meme
  // raisonnement que ensure_reference_conn). externalUpdatesEstimate vient
  // d'un sondage passif et bon marche (voir runExternalUpdatesEstimate,
  // declenche au montage) ; la verification/le telechargement complets
  // (couteux) ne se declenchent eux qu'au clic explicite sur le bouton --
  // jamais automatiquement, voir la discussion qui a mene a cette
  // fonctionnalite ("aucune maj ne se fait sans l'accord de l'utilisateur").
  const [externalUpdatesEstimate, setExternalUpdatesEstimate] = useState<CatchupRange | null>(null);
  const [externalUpdatesBusy, setExternalUpdatesBusy] = useState<"estimating" | "checking" | null>(null);
  const [externalUpdatesProgress, setExternalUpdatesProgress] = useState<string | null>(null);
  // Persiste dans localStorage (en plus de l'etat React) : rapporte par
  // l'utilisateur, l'ordinateur passe en veille pendant qu'il regarde la
  // revue (deja longue a obtenir -- plusieurs editions TWIC + recoupement
  // Lichess) a fait disparaitre la fenetre au reveil sans qu'il ait pu
  // annuler ni importer. Le PGN consolide sur disque (pgn_path) survit lui
  // sans probleme (chemin fixe, voir check_external_updates) ; seul le
  // resume affiche ici etait perdu. Restaure donc la revue au remontage du
  // composant plutot que de forcer l'utilisateur a tout retelecharger.
  const [externalUpdatesPreview, setExternalUpdatesPreview] = useState<UpdatePreview | null>(loadPersistedExternalUpdatesPreview);
  // Distingue le sondage passif (silencieux, juste pour le badge) du sondage
  // declenche par le clic utilisateur (qui doit, lui, enchainer sur la
  // confirmation puis la verification complete) -- les deux passent par le
  // meme evenement "external-updates-estimate-done".
  const externalUpdatesManualRef = useRef(false);
  const referenceBasePath = bases.find((b) => b.isReference && !b.missing)?.path ?? null;

  const startExternalUpdatesCheck = useCallback(() => {
    if (!referenceBasePath) return;
    setExternalUpdatesBusy("checking");
    setExternalUpdatesProgress(null);
    invoke("check_external_updates", { referenceDbPath: referenceBasePath }).catch((e) => {
      setExternalUpdatesBusy(null);
      setError(String(e));
    });
  }, [referenceBasePath]);

  const runExternalUpdatesEstimate = useCallback(
    (manual: boolean) => {
      if (!referenceBasePath) {
        if (manual) setError(t("home.externalUpdates.noReferenceBase"));
        return;
      }
      externalUpdatesManualRef.current = manual;
      if (manual) {
        setError(null);
        setExternalUpdatesBusy("estimating");
      }
      invoke("estimate_external_updates", { referenceDbPath: referenceBasePath }).catch((e) => {
        if (!manual) return;
        setExternalUpdatesBusy(null);
        setError(String(e));
      });
    },
    [referenceBasePath, t]
  );

  // Sondage silencieux au demarrage (et chaque fois que la base de reference
  // change) -- seulement de quoi afficher un badge, voir plus haut.
  useEffect(() => {
    runExternalUpdatesEstimate(false);
  }, [referenceBasePath]);

  useEffect(() => {
    const unlistenPromises = [
      listen<CatchupRange | null>("external-updates-estimate-done", async (event) => {
        setExternalUpdatesEstimate(event.payload);
        const manual = externalUpdatesManualRef.current;
        externalUpdatesManualRef.current = false;
        if (!manual) return;
        const range = event.payload;
        if (!range) {
          setExternalUpdatesBusy(null);
          await message(t("home.externalUpdates.upToDate"), { title: t("home.externalUpdates.title") });
          return;
        }
        if (range.weeksBehind >= LARGE_CATCHUP_WARNING_WEEKS) {
          setExternalUpdatesBusy(null);
          const confirmed = await ask(t("home.externalUpdates.largeCatchupConfirm", { weeks: range.weeksBehind }), {
            title: t("home.externalUpdates.title"),
            kind: "warning",
          });
          if (!confirmed) return;
        }
        startExternalUpdatesCheck();
      }),
      listen<string>("external-updates-estimate-error", (event) => {
        const manual = externalUpdatesManualRef.current;
        externalUpdatesManualRef.current = false;
        if (!manual) return;
        setExternalUpdatesBusy(null);
        setError(event.payload);
      }),
      listen<{ message: string }>("external-updates-progress", (event) => setExternalUpdatesProgress(event.payload.message)),
      listen<UpdatePreview | null>("external-updates-done", async (event) => {
        setExternalUpdatesBusy(null);
        setExternalUpdatesProgress(null);
        if (!event.payload) {
          await message(t("home.externalUpdates.upToDate"), { title: t("home.externalUpdates.title") });
          return;
        }
        localStorage.setItem(EXTERNAL_UPDATES_PREVIEW_KEY, JSON.stringify(event.payload));
        setExternalUpdatesPreview(event.payload);
      }),
      listen<string>("external-updates-error", (event) => {
        setExternalUpdatesBusy(null);
        setExternalUpdatesProgress(null);
        setError(event.payload);
      }),
      listen("menu-check-external-updates", () => runExternalUpdatesEstimate(true)),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [t, startExternalUpdatesCheck, runExternalUpdatesEstimate]);

  const handleCheckExternalUpdates = useCallback(() => {
    runExternalUpdatesEstimate(true);
  }, [runExternalUpdatesEstimate]);

  const dismissExternalUpdatesPreview = useCallback(() => {
    localStorage.removeItem(EXTERNAL_UPDATES_PREVIEW_KEY);
    setExternalUpdatesPreview(null);
  }, []);

  const handleConfirmExternalUpdatesImport = useCallback(
    (pgnPath: string) => {
      if (!referenceBasePath) return;
      dismissExternalUpdatesPreview();
      onImportExternalUpdates(referenceBasePath, pgnPath);
    },
    [referenceBasePath, dismissExternalUpdatesPreview, onImportExternalUpdates]
  );

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
    setPlayersPage(0);
    setPlayersSearchInput("");
    setPlayersSearch("");
    setPlayersSort(null);
    setTournamentsPage(0);
    setTournamentsSearchInput("");
    setTournamentsSearch("");
    setTournamentsSort(null);
  }, [activeBasePath]);

  const commitPlayersFilter = useCallback(() => {
    setPlayersSearch(playersSearchInput);
    setPlayersPage(0);
  }, [playersSearchInput]);

  const commitTournamentsFilter = useCallback(() => {
    setTournamentsSearch(tournamentsSearchInput);
    setTournamentsPage(0);
  }, [tournamentsSearchInput]);

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
            tournament: gamesAdvancedFilter.tournament,
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

  // meme logique de garde-fou que la liste des parties (voir gamesRequestIdRef) :
  // sans filtre de recherche, agreger les joueurs d'une base de plusieurs
  // millions de parties peut prendre plusieurs secondes.
  const playersRequestIdRef = useRef(0);
  useEffect(() => {
    if (!activeBasePath || activeBaseMissing) {
      setPlayers([]);
      setPlayersTotal(0);
      setPlayersLoading(false);
      return;
    }
    const requestId = ++playersRequestIdRef.current;
    setPlayersLoading(true);
    invoke<PlayersPage>("list_players_in_base", {
      path: activeBasePath,
      search: playersSearch,
      sortBy: playersSort?.column ?? "",
      sortDir: playersSort?.dir ?? "",
      offset: playersPage * PLAYERS_PAGE_SIZE,
      limit: PLAYERS_PAGE_SIZE,
    })
      .then((res) => {
        if (playersRequestIdRef.current !== requestId) return;
        setPlayers(res.players);
        setPlayersTotal(res.total);
      })
      .catch((e) => {
        if (playersRequestIdRef.current !== requestId) return;
        setError(String(e));
      })
      .finally(() => {
        if (playersRequestIdRef.current === requestId) setPlayersLoading(false);
      });
  }, [activeBasePath, activeBaseMissing, playersSearch, playersPage, playersSort]);

  const tournamentsRequestIdRef = useRef(0);
  useEffect(() => {
    if (!activeBasePath || activeBaseMissing) {
      setTournaments([]);
      setTournamentsTotal(0);
      setTournamentsLoading(false);
      return;
    }
    const requestId = ++tournamentsRequestIdRef.current;
    setTournamentsLoading(true);
    invoke<TournamentsPage>("list_tournaments_in_base", {
      path: activeBasePath,
      search: tournamentsSearch,
      sortBy: tournamentsSort?.column ?? "",
      sortDir: tournamentsSort?.dir ?? "",
      offset: tournamentsPage * TOURNAMENTS_PAGE_SIZE,
      limit: TOURNAMENTS_PAGE_SIZE,
    })
      .then((res) => {
        if (tournamentsRequestIdRef.current !== requestId) return;
        setTournaments(res.tournaments);
        setTournamentsTotal(res.total);
      })
      .catch((e) => {
        if (tournamentsRequestIdRef.current !== requestId) return;
        setError(String(e));
      })
      .finally(() => {
        if (tournamentsRequestIdRef.current === requestId) setTournamentsLoading(false);
      });
  }, [activeBasePath, activeBaseMissing, tournamentsSearch, tournamentsPage, tournamentsSort, tournamentsVersion]);

  // Modifier la cadence rejoue immediatement la liste (le nombre de parties
  // mises a jour figure dans la reponse, mais seule la cadence affichee doit
  // changer ici -- voir aussi tournamentsVersion dans les deps ci-dessus).
  const handleSetTournamentCadence = useCallback((tournamentId: number, timeControl: string) => {
    if (!activeBasePath) return;
    invoke("set_tournament_cadence", { path: activeBasePath, tournamentId, timeControl })
      .then(() => setTournamentsVersion((v) => v + 1))
      .catch((e) => setError(String(e)));
  }, [activeBasePath]);

  const handleFideRefresh = useCallback(async () => {
    setFideRefreshError(null);
    setFideRefreshProgress(0);
    setFideRefreshing(true);
    try {
      await invoke("refresh_fide_players");
    } catch (e) {
      setFideRefreshing(false);
      setFideRefreshError(String(e));
    }
  }, []);

  // Distinct de fideTotal (qui reflete la recherche/pagination en cours,
  // voir plus bas) : ce total-la est celui, non filtre, de toute la table,
  // affiche a cote de la date de derniere mise a jour. Declenche aussi
  // automatiquement une mise a jour si la derniere en local ne date pas du
  // mois en cours (voir isCurrentMonthFideUpdate) : pas besoin que
  // l'utilisateur ouvre le panneau FIDE et clique lui-meme. Sans risque de
  // boucle : apres un refresh, fide-refresh-done rappelle refreshFideStatus,
  // qui verra alors une date a jour et ne redeclenchera rien.
  const refreshFideStatus = useCallback(() => {
    invoke<FideStatus>("get_fide_status")
      .then((status) => {
        setFideLastUpdated(status.lastUpdated);
        if (!isCurrentMonthFideUpdate(status.lastUpdated)) {
          handleFideRefresh();
        }
      })
      .catch((e) => setError(String(e)));
  }, [handleFideRefresh]);

  useEffect(() => {
    refreshFideStatus();
  }, [refreshFideStatus]);

  const fideRequestIdRef = useRef(0);
  useEffect(() => {
    const requestId = ++fideRequestIdRef.current;
    setFideLoading(true);
    const yearInt = (s: string) => (s.trim() === "" ? undefined : parseInt(s, 10));
    invoke<FidePlayersPage>("list_fide_players", {
      search: fideSearch,
      sortBy: fideSort?.column ?? "",
      sortDir: fideSort?.dir ?? "",
      offset: fidePage * FIDE_PAGE_SIZE,
      limit: FIDE_PAGE_SIZE,
      fed: fideAdvancedFilter.fed.trim() === "" ? undefined : fideAdvancedFilter.fed.trim(),
      title: fideAdvancedFilter.title.trim() === "" ? undefined : fideAdvancedFilter.title.trim(),
      sex: fideAdvancedFilter.sex.trim() === "" ? undefined : fideAdvancedFilter.sex.trim(),
      status: fideAdvancedFilter.status === "" ? undefined : fideAdvancedFilter.status,
      birthYearMin: yearInt(fideAdvancedFilter.birthYearMin),
      birthYearMax: yearInt(fideAdvancedFilter.birthYearMax),
    })
      .then((res) => {
        if (fideRequestIdRef.current !== requestId) return;
        setFidePlayers(res.players);
        setFideTotal(res.total);
      })
      .catch((e) => {
        if (fideRequestIdRef.current !== requestId) return;
        setError(String(e));
      })
      .finally(() => {
        if (fideRequestIdRef.current === requestId) setFideLoading(false);
      });
  }, [fideSearch, fideAdvancedFilter, fidePage, fideSort]);

  useEffect(() => {
    const unlistenPromises = [
      listen<number>("fide-refresh-progress", (event) => setFideRefreshProgress(event.payload)),
      listen<{ count: number }>("fide-refresh-done", () => {
        setFideRefreshing(false);
        setFidePage(0);
        refreshFideStatus();
      }),
      listen<string>("fide-refresh-error", (event) => {
        setFideRefreshing(false);
        setFideRefreshError(event.payload);
      }),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [refreshFideStatus]);

  const handleSelectPlayer = useCallback(
    (name: string) => {
      const api = dockviewApi;
      setGamesAdvancedOpen(false);
      setGamesAdvancedInput(EMPTY_GAMES_ADVANCED_FILTER);
      setGamesAdvancedFilter(EMPTY_GAMES_ADVANCED_FILTER);
      setGamesSearchInput(name);
      setGamesSearch(name);
      setGamesPage(0);
      api?.getPanel("parties")?.api.setActive();
    },
    [dockviewApi]
  );

  // Clic sur un tournoi (onglet Tournois) : filtre le panneau Parties sur ce
  // tournoi exact -- passe par les filtres avances (recherche rapide et
  // filtres avances sont mutuellement exclusifs, voir onToggleGamesAdvanced)
  // plutot que la recherche rapide (reservee a Blancs/Noirs, voir
  // handleSelectPlayer) puisque le tournoi est un filtre avance a part entiere.
  const handleFilterGamesByTournament = useCallback(
    (name: string) => {
      const api = dockviewApi;
      setGamesSearchInput("");
      setGamesSearch("");
      const filter = { ...EMPTY_GAMES_ADVANCED_FILTER, tournament: name };
      setGamesAdvancedInput(filter);
      setGamesAdvancedFilter(filter);
      setGamesAdvancedOpen(true);
      setGamesPage(0);
      api?.getPanel("parties")?.api.setActive();
    },
    [dockviewApi]
  );

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
    (id: "bases" | "pgn" | "parties" | "players" | "fide" | "tournaments", visible: boolean) => {
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
        } else if (id === "parties") {
          const bases = api.getPanel("bases");
          api.addPanel({
            id: "parties",
            component: "parties",
            title: t("home.panel.games"),
            position: bases ? { direction: "right", referencePanel: "bases" } : undefined,
          });
        } else if (id === "players") {
          const parties = api.getPanel("parties");
          api.addPanel({
            id: "players",
            component: "players",
            title: t("home.panel.players"),
            position: parties ? { direction: "within", referencePanel: "parties" } : undefined,
          });
        } else if (id === "fide") {
          const players = api.getPanel("players");
          api.addPanel({
            id: "fide",
            component: "fide",
            title: t("home.panel.fide"),
            position: players ? { direction: "within", referencePanel: "players" } : undefined,
          });
        } else {
          const fide = api.getPanel("fide");
          api.addPanel({
            id: "tournaments",
            component: "tournaments",
            title: t("home.panel.tournaments"),
            position: fide ? { direction: "within", referencePanel: "fide" } : undefined,
          });
        }
      } else {
        existing?.api.close();
      }
    },
    [dockviewApi, t]
  );

  useEffect(() => {
    const unlistenPromises = (["bases", "pgn", "parties", "players", "fide", "tournaments"] as const).map((id) =>
      listen(`menu-toggle-panel-${id}`, () => {
        const isVisible = !!dockviewApi?.getPanel(id);
        setPanelVisible(id, !isVisible);
      })
    );
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [dockviewApi, setPanelVisible]);

  // Persiste la disposition (taille/position/ordre des panneaux, pas
  // seulement leur visibilite) a chaque changement, pour la retrouver telle
  // quelle a la prochaine ouverture de l'accueil.
  useEffect(() => {
    if (!dockviewApi) return;
    return persistDockviewLayout(dockviewApi, HOME_LAYOUT_KEY, HOME_PANEL_IDS);
  }, [dockviewApi]);

  const onDockviewReady = useCallback((event: DockviewReadyEvent) => {
    setDockviewApi(event.api);
    const api = event.api;
    if (tryRestoreDockviewLayout(api, HOME_LAYOUT_KEY)) {
      // une disposition sauvegardee avant l'ajout d'un panneau ne le
      // contient pas (fromJSON remplace entierement les panneaux) -- sans
      // ca, un panneau introduit apres coup (ex. "fide") resterait
      // introuvable pour toujours pour qui a deja une disposition
      // enregistree (voir le meme correctif applique au plateau, GameTab.tsx).
      if (!api.getPanel("fide")) {
        const playersPanel = api.getPanel("players");
        api.addPanel({
          id: "fide",
          component: "fide",
          title: t("home.panel.fide"),
          position: playersPanel ? { direction: "within", referencePanel: "players" } : undefined,
        });
      }
      if (!api.getPanel("tournaments")) {
        const fidePanel = api.getPanel("fide");
        api.addPanel({
          id: "tournaments",
          component: "tournaments",
          title: t("home.panel.tournaments"),
          position: fidePanel ? { direction: "within", referencePanel: "fide" } : undefined,
        });
      }
      syncPanelCheckedStates(api, HOME_PANEL_IDS);
      return;
    }
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
    if (prefs.players) {
      const partiesPanel = api.getPanel("parties");
      api.addPanel({
        id: "players",
        component: "players",
        title: t("home.panel.players"),
        position: partiesPanel ? { direction: "within", referencePanel: "parties" } : undefined,
      });
    }
    if (prefs.fide) {
      const playersPanel = api.getPanel("players");
      api.addPanel({
        id: "fide",
        component: "fide",
        title: t("home.panel.fide"),
        position: playersPanel ? { direction: "within", referencePanel: "players" } : undefined,
      });
    }
    if (prefs.tournaments) {
      const fidePanel = api.getPanel("fide");
      api.addPanel({
        id: "tournaments",
        component: "tournaments",
        title: t("home.panel.tournaments"),
        position: fidePanel ? { direction: "within", referencePanel: "fide" } : undefined,
      });
    }
    api.getPanel("bases")?.api.setActive();
    syncPanelCheckedStates(api, HOME_PANEL_IDS);
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
    players,
    playersTotal,
    playersLoading,
    playersPage,
    playersSearchInput,
    onPlayersSearchChange: setPlayersSearchInput,
    onPlayersPageChange: setPlayersPage,
    onPlayersFilterCommit: commitPlayersFilter,
    onSelectPlayer: handleSelectPlayer,
    playersSort,
    onPlayersSortChange,
    onPrepareAgainstPlayer,
    fidePlayers,
    fideTotal,
    fideLoading,
    fidePage,
    fideSearchInput,
    onFideSearchChange: setFideSearchInput,
    onFidePageChange: setFidePage,
    onFideFilterCommit: commitFideFilter,
    fideAdvancedOpen,
    onToggleFideAdvanced,
    fideAdvancedFilter: fideAdvancedInput,
    onFideAdvancedFilterChange,
    fideSort,
    onFideSortChange,
    fideLastUpdated,
    fideRefreshing,
    fideRefreshProgress,
    fideRefreshError,
    onFideRefresh: handleFideRefresh,
    tournaments,
    tournamentsTotal,
    tournamentsLoading,
    tournamentsPage,
    tournamentsSearchInput,
    onTournamentsSearchChange: setTournamentsSearchInput,
    onTournamentsPageChange: setTournamentsPage,
    onTournamentsFilterCommit: commitTournamentsFilter,
    tournamentsSort,
    onTournamentsSortChange,
    onSetTournamentCadence: handleSetTournamentCadence,
    onFilterGamesByTournament: handleFilterGamesByTournament,
    externalUpdatesEstimate,
    externalUpdatesBusy,
    onCheckExternalUpdates: handleCheckExternalUpdates,
  };

  return (
    <HomeContext.Provider value={contextValue}>
      <div className={"dockview-container " + theme.className}>
        <DockviewReact components={homeDockviewComponents} onReady={onDockviewReady} theme={theme} />
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
      {externalUpdatesBusy === "checking" && (
        <div className="modal-overlay">
          <div className="modal">
            <h2>{t("home.externalUpdates.title")}</h2>
            <p className="status-message">{externalUpdatesProgress ?? t("tools.working")}</p>
          </div>
        </div>
      )}
      {externalUpdatesPreview && (
        <ExternalUpdatesReviewDialog
          preview={externalUpdatesPreview}
          onConfirm={handleConfirmExternalUpdatesImport}
          onCancel={dismissExternalUpdatesPreview}
        />
      )}
    </HomeContext.Provider>
  );
}
