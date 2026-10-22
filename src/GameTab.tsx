import { forwardRef, useCallback, useEffect, useImperativeHandle, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { ask, save } from "@tauri-apps/plugin-dialog";
import { DockviewReact, type DockviewApi, type DockviewReadyEvent } from "dockview-react";
import type { Dests, Key } from "@lichess-org/chessground/types";
import { useTranslation } from "react-i18next";
import { AppContext, type AppContextValue, type TrainingFeedback, type TrainingSessionSummary } from "./AppContext";
import { dockviewComponents } from "./DockPanels";
import { persistDockviewLayout, syncPanelCheckedStates, tryRestoreDockviewLayout, useDockviewTheme } from "./dockviewPersistence";
import { nagExclusivityGroup } from "./nag";
import { SaveBasePicker } from "./components/SaveBasePicker";
import { useAutoDismissError } from "./useAutoDismissError";
import { useLatestFetch } from "./useLatestFetch";
import type {
  EcoInfo,
  EngineInfo,
  ExplorerFilterValues,
  ExplorerGameSummary,
  ExplorerGamesPage,
  ExplorerGamesSort,
  ExplorerGamesSortColumn,
  GameReportProgress,
  GameReportSummary,
  MoveDto,
  NodeDto,
  OpeningMoveStat,
  PvMove,
  SaveResult,
  ShapeDto,
  TabDto,
  TrainingJudgement,
  TrainingPrefetch,
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
  // Noeud sur lequel demarrer cet onglet (voir App.tsx/TabDto.initialNodeId) --
  // absent : demarre a la racine de l'arbre, comme avant.
  initialNodeId?: number;
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

// FEN de la racine de l'arbre -- envoye tel quel a opening_explorer/
// opening_explorer_games (voir GameSession/db::normalize_start_fen cote
// backend, qui se charge de reconnaitre une position standard) : l'appli ne
// se demande jamais elle-meme si cette partie a un depart personnalise
// (Nouvelle position, Chess960), c'est au backend de le determiner.
function rootFen(tree: TreeDto): string | undefined {
  return tree.nodes.find((n) => n.parent === null)?.fen;
}

interface GamePanelPrefs {
  notation: boolean;
  explorer: boolean;
  engine: boolean;
  training: boolean;
}

const GAME_PANELS_KEY = "gamePanels";
const GAME_LAYOUT_KEY = "gameDockviewLayout";
const GAME_LAYOUT_KEY_PREP = "gameDockviewLayoutPrep";
const GAME_PANEL_IDS = ["board", "notation", "explorer", "engine", "training"];

function loadGamePanelPrefs(): GamePanelPrefs {
  const defaults: GamePanelPrefs = { notation: true, explorer: false, engine: false, training: false };
  try {
    const raw = localStorage.getItem(GAME_PANELS_KEY);
    if (!raw) return defaults;
    return { ...defaults, ...JSON.parse(raw) };
  } catch {
    return defaults;
  }
}

export const GameTab = forwardRef<GameTabHandle, GameTabProps>(function GameTab(
  { tabId, active, isPlayerPrep = false, initialNodeId, onTreeChange },
  ref
) {
  const { t } = useTranslation();
  const [activeBasePath, setActiveBasePath] = useState<string | null>(null);
  const [showSaveBasePicker, setShowSaveBasePicker] = useState(false);
  const [isDirty, setIsDirty] = useState(false);
  const [closeConfirmOpen, setCloseConfirmOpen] = useState(false);
  const closeResolveRef = useRef<((proceed: boolean) => void) | null>(null);

  const [tree, setTree] = useState<TreeDto | null>(null);
  const [currentNodeId, setCurrentNodeId] = useState<number>(0);
  const [dests, setDests] = useState<Dests>(new Map());
  const [error, setError] = useAutoDismissError();
  const [variationChoice, setVariationChoice] = useState<NodeDto[] | null>(null);
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const [statusMessage, setStatusMessage] = useState<string | null>(null);
  const dockviewApiRef = useRef<DockviewApi | null>(null);
  const gameLayoutCleanupRef = useRef<(() => void) | null>(null);
  const explorerVisibilitySyncCleanupRef = useRef<(() => void) | null>(null);
  const theme = useDockviewTheme();
  const [boardTheme, setBoardThemeState] = useState(() => localStorage.getItem("boardTheme") ?? "brown");
  const setBoardTheme = useCallback((value: string) => {
    setBoardThemeState(value);
    localStorage.setItem("boardTheme", value);
  }, []);
  // Orientation de l'echiquier principal -- dans AppContext (pas locale au
  // panneau Echiquier) pour que l'apercu au survol d'une ligne du panneau
  // Moteur (EnginePanel, autre panneau dockview) s'affiche dans le meme sens
  // plutot que de revenir systematiquement aux Blancs en bas.
  const [boardFlipped, setBoardFlipped] = useState(false);
  // Une partie/etude demarrant d'une position personnalisee (tag FEN) au
  // trait des Noirs s'affiche par defaut echiquier retourne (Noirs en bas) --
  // plus naturel pour evaluer/etudier depuis le point de vue du camp au
  // trait, convention courante pour ce genre de position. Applique une seule
  // fois, au tout premier chargement de l'arbre (autoOrientedRef) : ne doit
  // jamais annuler un retournement manuel fait ensuite pendant qu'on avance
  // dans la partie (tree change a chaque coup).
  const autoOrientedRef = useRef(false);
  useEffect(() => {
    if (autoOrientedRef.current || !tree) return;
    autoOrientedRef.current = true;
    const root = tree.nodes.find((n) => n.parent === null);
    if (root?.turn === "black") setBoardFlipped(true);
  }, [tree]);

  // hydratation initiale de cet onglet (la session existe deja cote backend :
  // creee par l'accueil via new_game/open_game/import_pgn avant l'ouverture de l'onglet)
  useEffect(() => {
    invoke<TreeDto>("get_tree", { tabId }).then((t) => {
      setTree(t);
      // initialNodeId (voir App.tsx) : partie ouverte depuis l'explorateur,
      // demarre directement a la position recherchee plutot qu'a la racine.
      setCurrentNodeId(initialNodeId ?? t.root);
    });
    // si la partie vient d'une base (open_game), la sauvegarde doit
    // proposer cette meme base par defaut plutot que de repartir a vide
    invoke<string | null>("get_active_base_path", { tabId }).then(setActiveBasePath);
    // Rapport de partie deja calcule sur une session precedente (NAG +
    // "[%eval ...]" persistes avec la partie, voir apply_game_report cote
    // backend) : le reconstruit ici pour qu'il reste affiche a la
    // reouverture, au lieu de disparaitre tant qu'on ne relance pas
    // l'analyse. Repart de null si la ligne principale n'a pas ete analysee.
    invoke<GameReportSummary | null>("get_game_report", { tabId }).then(setGameReportSummary);
  }, [tabId, initialNodeId]);

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
    async (from: Key, to: Key, promotion: string | null = null) => {
      try {
        const newId = await invoke<number>("play_move", {
          tabId,
          nodeId: currentNodeId,
          from,
          to,
          promotion,
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

  // Clic sur un coup d'une variante du moteur (EnginePanel) : rejoue tous les
  // coups de la ligne jusqu'a celui clique (inclus) depuis la position
  // actuellement analysee (currentNodeId -- toujours celle de analyzedFen,
  // voir DockPanels.tsx::EnginePanelView, l'analyse redemarre a chaque
  // changement de position). play_move reutilise un enfant existant si le
  // coup y figure deja (meme logique qu'un coup joue normalement sur
  // l'echiquier), donc rejouer une ligne deja presente dans l'arbre se
  // contente de s'y deplacer plutot que de dupliquer la variation.
  const handlePlayPvLine = useCallback(
    async (moves: PvMove[], uptoIndex: number) => {
      try {
        let nodeId = currentNodeId;
        for (let i = 0; i <= uptoIndex; i++) {
          const move = moves[i];
          nodeId = await invoke<number>("play_move", {
            tabId,
            nodeId,
            from: move.from,
            to: move.to,
            promotion: move.promotion,
          });
        }
        const newTree = await invoke<TreeDto>("get_tree", { tabId });
        setTree(newTree);
        setCurrentNodeId(nodeId);
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

  // Navigation dans l'arbre, factorisee pour etre partagee par les
  // raccourcis clavier (fleches) et les boutons de transport sous
  // l'echiquier -- meme logique, deux declencheurs.
  const handleStep = useCallback(
    (direction: "start" | "prev" | "next" | "end") => {
      if (!tree) return;
      const byId = new Map(tree.nodes.map((n) => [n.id, n]));

      if (direction === "start") {
        setCurrentNodeId(tree.root);
        setVariationChoice(null);
        return;
      }
      if (direction === "end") {
        let node = byId.get(tree.root);
        while (node && node.children.length > 0) {
          node = byId.get(node.children[0]);
        }
        if (node) setCurrentNodeId(node.id);
        setVariationChoice(null);
        return;
      }

      const node = byId.get(currentNodeId);
      if (!node) return;
      if (direction === "prev" && node.parent !== null) {
        setCurrentNodeId(node.parent);
        setVariationChoice(null);
      } else if (direction === "next" && node.children.length > 0) {
        if (node.children.length === 1) {
          setCurrentNodeId(node.children[0]);
          setVariationChoice(null);
        } else {
          const options = node.children
            .map((childId) => byId.get(childId))
            .filter((n): n is NodeDto => n !== undefined);
          setHighlightedIndex(0);
          setVariationChoice(options);
        }
      }
    },
    [tree, currentNodeId]
  );

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
      // Un seul NAG actif a la fois par sous-groupe d'exclusivite (voir
      // nag.ts::nagExclusivityGroup) -- ex. activer "espace decisif Blancs"
      // desactive "espace leger Blancs" (meme echelle), mais pas "roi bien
      // place Blancs" (echelle independante, meme si les deux sont
      // regroupes sous le meme bouton dans la barre d'annotation).
      const group = nagExclusivityGroup(code);
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

  const handlePromoteVariation = useCallback(async () => {
    try {
      await invoke("promote_variation", { tabId, nodeId: currentNodeId });
      await refreshTree();
      setIsDirty(true);
    } catch (e) {
      setError(String(e));
    }
  }, [tabId, currentNodeId, refreshTree]);

  const handleDeleteVariation = useCallback(async () => {
    try {
      const parentId = await invoke<number>("delete_variation", { tabId, nodeId: currentNodeId });
      await refreshTree();
      setCurrentNodeId(parentId);
      setIsDirty(true);
    } catch (e) {
      setError(String(e));
    }
  }, [tabId, currentNodeId, refreshTree]);

  // Bouton "type de jeu" (transport, panneau Echiquier) : remplace la partie
  // de CET onglet par une nouvelle partie Chess960 -- pas un nouvel onglet,
  // contrairement a Fichier > Nouvelle position. Demande confirmation des
  // qu'au moins un coup a ete joue, comme toute action qui jetterait la
  // partie en cours.
  const handleNewChess960 = useCallback(
    async (positionNumber: number) => {
      if (tree && tree.nodes.length > 1) {
        const proceed = await ask(t("gameTab.chess960ConfirmReplace"), {
          title: t("gameTab.chess960ConfirmTitle"),
          kind: "warning",
        });
        if (!proceed) return;
      }
      try {
        await invoke("new_chess960_game", { tabId, positionNumber });
        await refreshTree();
        setCurrentNodeId(0);
        setActiveBasePath(null);
        setIsDirty(false);
        setGameReportSummary(null);
        setError(null);
      } catch (e) {
        setError(String(e));
      }
    },
    [tabId, tree, refreshTree, t]
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
  const [engineError, setEngineError] = useAutoDismissError();

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

  // --- Rapport de partie ---
  const [gameReportRunning, setGameReportRunning] = useState(false);
  const [gameReportProgress, setGameReportProgress] = useState<GameReportProgress | null>(null);
  const [gameReportSummary, setGameReportSummary] = useState<GameReportSummary | null>(null);
  const [gameReportError, setGameReportError] = useAutoDismissError();

  useEffect(() => {
    if (!gameReportRunning) return;
    const unlistenPromises = [
      listen<GameReportProgress>(`game-report-progress-${tabId}`, (event) => {
        setGameReportProgress(event.payload);
      }),
      listen<GameReportSummary>(`game-report-done-${tabId}`, async (event) => {
        setGameReportRunning(false);
        setGameReportProgress(null);
        setGameReportSummary(event.payload);
        await refreshTree();
        setIsDirty(true);
      }),
      listen<string>(`game-report-error-${tabId}`, (event) => {
        setGameReportRunning(false);
        setGameReportProgress(null);
        setGameReportError(event.payload);
      }),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [tabId, gameReportRunning, refreshTree]);

  const handleStartGameReport = useCallback(
    async (depth: number, threads: number) => {
      if (!enginePath) {
        setGameReportError(t("gameReport.noEnginePath"));
        return;
      }
      setGameReportError(null);
      setGameReportSummary(null);
      setGameReportProgress({ done: 0, total: 0 });
      setGameReportRunning(true);
      try {
        await invoke("start_game_report", { tabId, enginePath, depth, threads });
      } catch (e) {
        setGameReportRunning(false);
        setGameReportProgress(null);
        setGameReportError(String(e));
      }
    },
    [tabId, enginePath, t]
  );

  // --- Mode entrainement ---
  // Position courante de l'exercice, independante de currentNodeId (la
  // navigation normale de l'echiquier/notation) : l'utilisateur doit pouvoir
  // continuer a parcourir la partie normalement pendant qu'un entrainement
  // est en pause sur une autre position, voir ExplorerPreview pour le meme
  // principe (previewNodeId separe de currentNodeId).
  const [trainingNodeId, setTrainingNodeId] = useState<number | null>(null);
  // camp pour lequel l'exercice a demarre (voir handleStartTraining) : fixe
  // pour toute la duree de l'exercice, les coups de l'autre camp sont
  // rejoues automatiquement depuis la partie sans etre juges (l'utilisateur
  // ne les choisit pas).
  const [trainingSide, setTrainingSide] = useState<"white" | "black" | null>(null);
  const [trainingFeedback, setTrainingFeedback] = useState<TrainingFeedback | null>(null);
  const [trainingBusy, setTrainingBusy] = useState(false);
  const [trainingError, setTrainingError] = useAutoDismissError();
  const [trainingDests, setTrainingDests] = useState<Dests>(new Map());
  // Un coup juge (correct ou non) par round, dans l'ordre -- pour le score de
  // precision cumule (affiche en continu, voir TrainingPanel) et le resume de
  // fin de session (voir trainingSummary plus bas). "classification" null
  // couvre a la fois un coup correct (accuracy 100) et un coup different mais
  // pas clairement pire (voir judge_training_move cote backend).
  const [trainingRounds, setTrainingRounds] = useState<{ classification: number | null; accuracy: number }[]>([]);
  const [trainingSummary, setTrainingSummary] = useState<TrainingSessionSummary | null>(null);
  // Evaluation de la position actuelle + de celle du coup reel, lancee des
  // que c'est au tour du trainee de jouer (donc PENDANT qu'il reflechit, pas
  // apres qu'il ait joue) -- voir l'effet de pre-chargement plus bas et son
  // utilisation dans handleTrainingMove.
  const [trainingPrefetch, setTrainingPrefetch] = useState<TrainingPrefetch | null>(null);
  // Reglage PROPRE a l'entrainement (pas celui, invisible ici, du dialogue
  // "Rapport de partie" repris silencieusement au depart) : l'utilisateur
  // n'a aucun moyen de voir/changer ce dernier depuis l'onglet Entrainement,
  // "changer la profondeur" n'avait donc aucun effet observable -- rapporte
  // comme "le moteur reste trop lent meme en profondeur 10". Visible et
  // reglable directement dans TrainingPanel, persiste sous ses propres cles.
  const [trainingDepth, setTrainingDepthState] = useState(() => Number(localStorage.getItem("trainingDepth")) || 14);
  const [trainingThreadsSetting, setTrainingThreadsSettingState] = useState(
    () => Number(localStorage.getItem("trainingThreads")) || 1
  );
  const setTrainingDepth = useCallback((depth: number) => {
    setTrainingDepthState(depth);
    localStorage.setItem("trainingDepth", String(depth));
  }, []);
  const setTrainingThreadsSetting = useCallback((threads: number) => {
    setTrainingThreadsSettingState(threads);
    localStorage.setItem("trainingThreads", String(threads));
  }, []);

  const trainingNode = tree?.nodes.find((n) => n.id === trainingNodeId) ?? null;
  const trainingFen = trainingNode?.fen ?? null;
  const trainingAtEnd = trainingNode !== null && trainingNode.children.length === 0;
  const trainingAccuracy = useMemo(
    () => (trainingRounds.length === 0 ? null : trainingRounds.reduce((sum, r) => sum + r.accuracy, 0) / trainingRounds.length),
    [trainingRounds]
  );

  // nodeId du dernier pre-chauffage LANCE (pas forcement termine) : evite de
  // relancer invoke() a chaque re-render tant que l'evenement de reponse
  // n'est pas arrive, voir l'effet de pre-chargement plus bas.
  const trainingPrefetchRequestedRef = useRef<number | null>(null);
  // Contexte (SAN du coup reel, noeud vers lequel avancer) du jugement en
  // cours, pose juste avant l'appel a judge_training_move et consomme par
  // l'ecouteur "training-judge-done" -- le backend ne renvoie que le
  // verdict, pas ces informations propres a l'arbre cote frontend.
  const trainingPendingJudgeRef = useRef<{ nodeId: number; gameSan: string; pendingNodeId: number } | null>(null);
  // Derniere position d'entrainement connue, lue par les ecouteurs
  // d'evenements (effet stable, abonne une seule fois par tabId, voir plus
  // bas) pour ignorer une reponse perimee sans avoir a se reabonner a
  // chaque coup.
  const trainingNodeIdRef = useRef<number | null>(null);
  trainingNodeIdRef.current = trainingNodeId;

  const handleStartTraining = useCallback(
    (nodeId: number) => {
      const node = tree?.nodes.find((n) => n.id === nodeId);
      if (!node) return;
      setTrainingNodeId(nodeId);
      setTrainingSide(node.turn);
      setTrainingFeedback(null);
      setTrainingError(null);
      setTrainingRounds([]);
      setTrainingSummary(null);
      setTrainingPrefetch(null);
      trainingPrefetchRequestedRef.current = null;
      trainingPendingJudgeRef.current = null;
    },
    [tree]
  );

  const handleStopTraining = useCallback(() => {
    setTrainingNodeId(null);
    setTrainingSide(null);
    setTrainingFeedback(null);
    setTrainingRounds([]);
    setTrainingSummary(null);
    setTrainingPrefetch(null);
    trainingPrefetchRequestedRef.current = null;
    trainingPendingJudgeRef.current = null;
  }, []);

  // coups legaux de l'exercice : uniquement quand c'est au trainee de jouer
  // (sinon le coup suivant est rejoue automatiquement, voir l'effet
  // ci-dessous), qu'aucun retour n'est affiche (il faut d'abord "Continuer"
  // avant de pouvoir rejouer, voir handleTrainingContinue) ET qu'aucun
  // jugement n'est en cours (trainingBusy) -- sans ce dernier garde-fou,
  // l'echiquier redevenait jouable des que play_move avait insere la
  // variation (tout de suite, voir handleTrainingMove), AVANT meme que le
  // moteur ait fini d'analyser le coup : l'utilisateur pouvait alors rejouer
  // par-dessus une analyse encore en cours. Le coup ne doit redevenir
  // reprenable qu'une fois la reflexion (l'analyse) terminee.
  useEffect(() => {
    if (!trainingNode || trainingNode.turn !== trainingSide || trainingFeedback || trainingBusy) {
      setTrainingDests(new Map());
      return;
    }
    invoke<MoveDto[]>("legal_moves", { tabId, nodeId: trainingNode.id })
      .then((moves) => setTrainingDests(buildDests(moves)))
      .catch(() => setTrainingDests(new Map()));
  }, [tabId, trainingNode, trainingSide, trainingFeedback, trainingBusy]);

  // rejoue automatiquement le coup de l'autre camp (celui que l'utilisateur
  // ne controle pas dans cet exercice) : petit delai pour qu'il reste
  // visible au lieu de sauter instantanement a la position suivante.
  useEffect(() => {
    if (!trainingNode || trainingNode.turn === trainingSide || trainingNode.children.length === 0) return;
    const nextId = trainingNode.children[0];
    const timer = setTimeout(() => setTrainingNodeId(nextId), 350);
    return () => clearTimeout(timer);
  }, [trainingNode, trainingSide]);

  // Fin de ligne atteinte avec au moins un coup juge pendant la session :
  // affiche le resume (voir TrainingPanel) au lieu de rester sur "fin de la
  // ligne". Ne se redeclenche pas (garde trainingSummary) une fois pose.
  useEffect(() => {
    if (trainingAtEnd && trainingRounds.length > 0 && !trainingSummary) {
      const blunders = trainingRounds.filter((r) => r.classification === 4).length;
      const mistakes = trainingRounds.filter((r) => r.classification === 2).length;
      const inaccuracies = trainingRounds.filter((r) => r.classification === 6).length;
      setTrainingSummary({
        accuracy: trainingRounds.reduce((sum, r) => sum + r.accuracy, 0) / trainingRounds.length,
        blunders,
        mistakes,
        inaccuracies,
        rounds: trainingRounds.length,
      });
    }
  }, [trainingAtEnd, trainingRounds, trainingSummary]);

  // judge_training_move/prefetch_training_judgement tournent sur un thread
  // dedie cote Rust et renvoient leur resultat par evenement (voir ces deux
  // commandes, lib.rs) -- jamais directement comme valeur de retour de
  // invoke(). Un premier essai synchrone (commande bloquant jusqu'au
  // resultat, meme a un seul process moteur a la fois) gelait l'appli
  // entiere pendant toute l'analyse : c'est le fait de bloquer le HANDLER de
  // la commande qui bloque la fenetre, pas la charge CPU du moteur -- exact
  // probleme deja rencontre et corrige pour le rapport de partie
  // (start_game_report), reproduit ici par erreur puis corrige de la meme
  // facon.
  useEffect(() => {
    const unlistenPromises = [
      listen<TrainingPrefetch>(`training-prefetch-done-${tabId}`, (event) => {
        if (trainingNodeIdRef.current === event.payload.nodeId) setTrainingPrefetch(event.payload);
      }),
      listen<string>(`training-prefetch-error-${tabId}`, () => {
        // simple pre-chauffage best-effort : judge_training_move retombera
        // sur son calcul complet (3 positions) sans rien perdre.
      }),
      listen<TrainingJudgement>(`training-judge-done-${tabId}`, (event) => {
        const pending = trainingPendingJudgeRef.current;
        trainingPendingJudgeRef.current = null;
        setTrainingBusy(false);
        if (!pending || trainingNodeIdRef.current !== pending.nodeId) return;
        setTrainingRounds((prev) => [...prev, { classification: event.payload.classification, accuracy: event.payload.accuracy }]);
        setTrainingFeedback({ ...event.payload, gameSan: pending.gameSan, pendingNodeId: pending.pendingNodeId });
      }),
      listen<string>(`training-judge-error-${tabId}`, (event) => {
        trainingPendingJudgeRef.current = null;
        setTrainingBusy(false);
        setTrainingError(event.payload);
      }),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [tabId]);

  // Pre-chauffage moteur : des que c'est au trainee de jouer, evalue deja la
  // position actuelle ET celle du coup reel, AVANT meme de savoir quel coup
  // sera essaye -- pour que le moteur mette a profit le temps de reflexion de
  // l'utilisateur plutot que de ne demarrer qu'une fois le coup joue (c'etait
  // le cas avant, rapporte comme trop lent a l'usage). judge_training_move
  // n'a plus ensuite qu'UNE position de plus a evaluer (voir handleTrainingMove).
  useEffect(() => {
    if (!trainingNode || trainingNode.turn !== trainingSide || trainingFeedback || !enginePath) return;
    if (trainingPrefetch?.nodeId === trainingNode.id) return;
    if (trainingPrefetchRequestedRef.current === trainingNode.id) return;
    const gameChildId = trainingNode.children[0];
    const gameChild = gameChildId !== undefined ? tree?.nodes.find((n) => n.id === gameChildId) : undefined;
    if (!gameChild || !gameChild.from || !gameChild.to) return;
    trainingPrefetchRequestedRef.current = trainingNode.id;
    invoke("prefetch_training_judgement", {
      tabId,
      nodeId: trainingNode.id,
      enginePath,
      depth: trainingDepth,
      threads: trainingThreadsSetting,
      fenBefore: trainingNode.fen,
      gameFrom: gameChild.from,
      gameTo: gameChild.to,
      gamePromotion: gameChild.promotion,
    }).catch(() => {
      // echec immediat (ex. chemin moteur invalide) : permet un nouvel essai
      // plus tard plutot que de rester bloque sur ce nodeId pour toujours.
      trainingPrefetchRequestedRef.current = null;
    });
  }, [trainingNode, trainingSide, trainingFeedback, enginePath, tree, trainingPrefetch, trainingDepth, trainingThreadsSetting, tabId]);

  const handleTrainingMove = useCallback(
    async (from: Key, to: Key, promotion: string | null = null) => {
      if (!trainingNode) return;
      const gameChildId = trainingNode.children[0];
      const gameChild = gameChildId !== undefined ? tree?.nodes.find((n) => n.id === gameChildId) : undefined;
      if (!gameChild || !gameChild.from || !gameChild.to) return;

      if (from === gameChild.from && to === gameChild.to && promotion === gameChild.promotion) {
        // Rejouer le coup de la partie ne garantit pas que c'etait le
        // meilleur coup : si le pre-chauffage de cette position a deja
        // identifie mieux (voir prefetch_training_judgement), on le signale
        // aussi ici plutot que de se limiter a un "coup de la partie !"
        // silencieux sur un coup pourtant imparfait.
        const prefetched = trainingPrefetch?.nodeId === trainingNode.id ? trainingPrefetch : null;
        const betterGameMoveSan = prefetched?.betterGameMoveSan ?? null;
        setTrainingRounds((prev) => [...prev, { classification: null, accuracy: 100 }]);
        setTrainingFeedback({
          matchesGame: true,
          gameSan: gameChild.san ?? "",
          classification: null,
          gameMoveComparison: "similar",
          refutation: [],
          accuracy: 100,
          betterGameMoveSan,
          gameMoveClassification: prefetched?.gameMoveClassification ?? null,
          pendingNodeId: gameChild.id,
        });
        if (!betterGameMoveSan) {
          // coup de la partie deja optimal : avancee automatique, rien de
          // plus a lire. Sinon on attend "Continuer" (voir handleTrainingContinue).
          setTimeout(() => {
            setTrainingFeedback(null);
            setTrainingNodeId(gameChild.id);
          }, 500);
        }
        return;
      }
      if (!enginePath) {
        setTrainingError(t("gameReport.noEnginePath"));
        return;
      }
      setTrainingBusy(true);
      setTrainingError(null);
      try {
        // Insere le coup essaye comme variation dans la VRAIE partie (comme un
        // coup normal sur l'echiquier, voir handleMove) : contrairement a
        // judge_training_move (qui ne fait que l'evaluer, sans toucher a la
        // session), l'utilisateur doit pouvoir retrouver ses essais ensuite
        // dans la Notation, pas seulement un verdict ephemere ici.
        await invoke<number>("play_move", { tabId, nodeId: trainingNode.id, from, to, promotion });
        const newTree = await invoke<TreeDto>("get_tree", { tabId });
        setTree(newTree);
        setIsDirty(true);

        // Reutilise le pre-chauffage de cette position s'il a deja abouti ;
        // sinon judge_training_move refait le calcul complet (3 positions)
        // de son cote -- plus de "promesse en vol" a attendre ici, tout
        // tourne desormais sur un thread separe (voir l'effet d'ecoute
        // ci-dessus), donc rien a bloquer.
        const prefetched = trainingPrefetch?.nodeId === trainingNode.id ? trainingPrefetch : null;
        trainingPendingJudgeRef.current = { nodeId: trainingNode.id, gameSan: gameChild.san ?? "", pendingNodeId: gameChild.id };
        await invoke("judge_training_move", {
          tabId,
          enginePath,
          depth: trainingDepth,
          threads: trainingThreadsSetting,
          fenBefore: trainingNode.fen,
          userFrom: from,
          userTo: to,
          userPromotion: promotion,
          gameFrom: gameChild.from,
          gameTo: gameChild.to,
          gamePromotion: gameChild.promotion,
          evalBefore: prefetched?.evalBefore ?? null,
          evalGame: prefetched?.evalGame ?? null,
        });
        // le verdict arrive via l'evenement "training-judge-done-{tabId}"
        // (effet d'ecoute plus haut), qui remet aussi trainingBusy a false.
      } catch (e) {
        trainingPendingJudgeRef.current = null;
        setTrainingError(String(e));
        setTrainingBusy(false);
      }
    },
    [trainingNode, tree, tabId, enginePath, trainingPrefetch, trainingDepth, trainingThreadsSetting, t]
  );

  const handleTrainingContinue = useCallback(() => {
    if (trainingFeedback?.pendingNodeId != null) {
      setTrainingNodeId(trainingFeedback.pendingNodeId);
    }
    setTrainingFeedback(null);
  }, [trainingFeedback]);

  const setPanelVisible = useCallback((id: "board" | "notation" | "explorer" | "engine" | "training", visible: boolean) => {
    if (id === "explorer") setExplorerVisible(visible);
    const api = dockviewApiRef.current;
    if (!api) return;
    const existing = api.getPanel(id);
    if (visible) {
      if (existing) {
        existing.api.setActive();
        return;
      }
      if (id === "board") {
        // pas de `position` : redevient le panneau d'ancrage, comme au tout
        // premier onglet (voir onDockviewReady) -- l'echiquier n'a pas de
        // "referencePanel" naturel puisque c'est lui que les autres visent.
        api.addPanel({ id: "board", component: "board", title: t("gameTab.panel.board") });
      } else if (id === "notation") {
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
      } else if (id === "engine") {
        const notation = api.getPanel("notation");
        api.addPanel({
          id: "engine",
          component: "engine",
          title: t("gameTab.panel.engine"),
          position: notation ? { direction: "below", referencePanel: "notation" } : undefined,
        });
      } else {
        // onglet dedie, regroupe avec Notation/Explorateur (voir la carte
        // Trello "Mode d'entrainement") plutot qu'un panneau separe.
        const notation = api.getPanel("notation");
        api.addPanel({
          id: "training",
          component: "training",
          title: t("gameTab.panel.training"),
          position: notation ? { direction: "within", referencePanel: "notation" } : undefined,
        });
      }
    } else {
      existing?.api.close();
    }
  }, [t]);

  const toggleGamePanel = useCallback(
    (id: "board" | "notation" | "explorer" | "engine" | "training") => {
      const isVisible = !!dockviewApiRef.current?.getPanel(id);
      setPanelVisible(id, !isVisible);
    },
    [setPanelVisible]
  );

  // le moteur s'arrete quand cet onglet se ferme (idem pour le process
  // moteur persistant du mode entrainement, voir BatchEngine/engine.rs)
  useEffect(() => {
    return () => {
      invoke("stop_engine", { tabId }).catch(() => {});
      invoke("stop_training_engine", { tabId }).catch(() => {});
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
  // Distingue "en cours de chargement" de "vraiment aucune partie a cette
  // position" (voir OpeningExplorer, explorer.empty) : sans ca, le premier
  // chargement (ex. juste apres "Se preparer contre", explorerVisible
  // devenant vrai des l'ouverture) affichait le message "aucune partie"
  // pendant l'attente, avant meme que la reponse soit revenue -- rapporte
  // par l'utilisateur comme trompeur.
  const [explorerStatsLoading, setExplorerStatsLoading] = useState(false);
  const [explorerError, setExplorerError] = useAutoDismissError();
  // Vrai seulement quand l'onglet Explorateur est reellement au premier
  // plan (voir onDockviewReady, onDidActivePanelChange) -- pas juste
  // "present quelque part dans la disposition" : gate les deux requetes
  // par coup plus bas (opening_explorer/opening_explorer_games), inutile
  // de les payer pour un onglet existant mais pas regarde. Valeur initiale
  // ici juste une estimation avant que dockview ne soit pret ; corrigee
  // tout de suite apres (onDockviewReady).
  const [explorerVisible, setExplorerVisible] = useState(() => isPlayerPrep || loadGamePanelPrefs().explorer);
  // Nettoyage en tache de fond du "+"/"#" laisse par un import en masse
  // anterieur a la correction de bulk_import (voir maybe_start_check_suffix_cleanup,
  // lib.rs) : evenement global (pas propre a cet onglet), tant que non nul
  // affiche que les resultats de l'explorateur peuvent encore etre
  // incomplets pour un coup donnant echec/mat.
  const [checkSuffixesCleanupCount, setCheckSuffixesCleanupCount] = useState<number | null>(null);

  useEffect(() => {
    const unlistenPromises = [
      listen<number>("check-suffixes-progress", (event) => setCheckSuffixesCleanupCount(event.payload)),
      listen("check-suffixes-done", () => setCheckSuffixesCleanupCount(null)),
      // ne disparaissait auparavant pas autrement qu'en silence, exactement
      // comme un succes -- aucun moyen de savoir qu'un nettoyage s'etait
      // arrete en cours de route (voir maybe_start_check_suffix_cleanup,
      // lib.rs). Relancer l'appli redeclenche automatiquement une nouvelle
      // tentative (le marqueur "termine" n'a jamais ete pose en cas d'echec).
      listen<string>("check-suffixes-error", (event) => {
        setCheckSuffixesCleanupCount(null);
        setError(t("explorer.checkSuffixCleanupError", { error: event.payload }));
      }),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [t]);

  const EMPTY_EXPLORER_FILTERS: ExplorerFilterValues = {
    playerName: "",
    playerWhite: true,
    ratingMin: "",
    ratingMax: "",
    yearMin: "",
    yearMax: "",
  };
  // explorerFiltersInput (saisie libre, aucune requete declenchee) vs
  // explorerFilters (valeurs validees, celles reellement envoyees a
  // opening_explorer/opening_explorer_games) : meme principe que
  // gamesAdvancedInput/gamesAdvancedFilter (HomeDockview.tsx) -- interroger
  // la base de reference a chaque frappe rendait l'interface saccadee et,
  // sur un nom, se comportait comme une recherche en temps reel bien trop
  // lente sur plusieurs millions de parties.
  const [explorerFiltersInput, setExplorerFiltersInput] = useState<ExplorerFilterValues>(EMPTY_EXPLORER_FILTERS);
  const [explorerFilters, setExplorerFilters] = useState<ExplorerFilterValues>(EMPTY_EXPLORER_FILTERS);
  const commitExplorerFilters = useCallback(() => setExplorerFilters(explorerFiltersInput), [explorerFiltersInput]);
  const [ecoInfo, setEcoInfo] = useState<EcoInfo | null>(null);

  // "" cote UI (champ vide) -> undefined cote commande Rust (pas de filtre) ;
  // classement/annee ne s'appliquent que par paire min+max (voir
  // build_explorer_filters, lib.rs) -- un seul bord rempli sans l'autre
  // n'envoie donc rien, plutot que de deviner une borne manquante.
  function explorerFilterArgs(f: ExplorerFilterValues) {
    const int = (s: string) => (s.trim() === "" ? undefined : parseInt(s, 10));
    return {
      playerName: f.playerName.trim() === "" ? undefined : f.playerName.trim(),
      playerWhite: f.playerWhite,
      ratingMin: int(f.ratingMin),
      ratingMax: int(f.ratingMax),
      yearMin: int(f.yearMin),
      yearMax: int(f.yearMax),
    };
  }

  // ne pas interroger la base de reference tant que le panneau n'est pas
  // affiche : sur une base de plusieurs millions de parties, les premiers
  // coups d'une partie restent lents a agreger (voir opening_explorer cote
  // Rust), inutile de payer ce cout a chaque coup si personne ne regarde.
  // useLatestFetch (pas un useEffect direct) : jouer les coups vite
  // enchainait sinon une requete par coup, l'explorateur accusant un
  // retard perceptible sur l'echiquier (rapporte par l'utilisateur) --
  // ici, un seul appel a la fois, toujours pour la position la plus
  // recente une fois le precedent revenu.
  useLatestFetch(
    !!tree && explorerVisible,
    [tabId, tree, currentNodeId, explorerVisible, explorerFilters],
    () => {
      setExplorerStatsLoading(true);
      const moves = pathSans(tree!, currentNodeId);
      return invoke<OpeningMoveStat[]>("opening_explorer", { tabId, moves, startFen: rootFen(tree!), ...explorerFilterArgs(explorerFilters) });
    },
    (stats) => {
      setExplorerStats(stats);
      setExplorerStatsLoading(false);
      setExplorerError(null);
    },
    (e) => {
      setExplorerStatsLoading(false);
      setExplorerError(String(e));
    }
  );

  useEffect(() => {
    // Classification ECO : cout negligeable (recherche en memoire dans un
    // referentiel embarque, voir eco.rs), independante de la base de
    // reference -- pas besoin de la conditionner a explorerVisible.
    if (!tree) return;
    const moves = pathSans(tree, currentNodeId);
    invoke<EcoInfo | null>("classify_opening", { moves })
      .then(setEcoInfo)
      .catch(() => setEcoInfo(null));
  }, [tree, currentNodeId]);

  const handleExplorerMove = useCallback(
    async (san: string) => {
      try {
        // s.san (voir OpeningExplorer) vient de mainline_sans, qui peut
        // encore porter le suffixe "+"/"#" d'un coup donnant echec/mat (bases
        // importees avant la correction cote bulk_import) -- legal_moves,
        // lui, ne l'a jamais (voir game_tree::legal_moves). Sans ce retrait,
        // le clic ne trouvait jamais de correspondance et ne faisait rien.
        const cleanSan = san.replace(/[+#]$/, "");
        const moves = await invoke<MoveDto[]>("legal_moves", { tabId, nodeId: currentNodeId });
        const match = moves.find((m) => m.san === cleanSan);
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
  const [explorerGames, setExplorerGames] = useState<ExplorerGameSummary[]>([]);
  const [explorerGamesTotal, setExplorerGamesTotal] = useState(0);
  const [explorerGamesPage, setExplorerGamesPage] = useState(0);
  const [explorerGamesSort, setExplorerGamesSort] = useState<ExplorerGamesSort | null>(null);
  const [explorerGamesLoading, setExplorerGamesLoading] = useState(false);
  const [explorerGamesError, setExplorerGamesError] = useAutoDismissError();
  const [explorerGamesBasePath, setExplorerGamesBasePath] = useState<string | null>(null);

  const onExplorerGamesSortChange = useCallback((column: ExplorerGamesSortColumn) => {
    setExplorerGamesSort((prev) => (prev?.column === column ? { column, dir: prev.dir === "asc" ? "desc" : "asc" } : { column, dir: "asc" }));
    setExplorerGamesPage(0);
  }, []);

  const [previewGameId, setPreviewGameId] = useState<number | null>(null);
  const [previewTree, setPreviewTree] = useState<TreeDto | null>(null);
  const [previewNodeId, setPreviewNodeId] = useState<number | null>(null);
  const [previewError, setPreviewError] = useAutoDismissError();

  const handleClosePreview = useCallback(() => {
    setPreviewGameId(null);
    setPreviewTree(null);
    setPreviewNodeId(null);
    setPreviewError(null);
  }, []);

  // changer de position dans l'arbre ou de filtre invalide la liste/l'aperçu
  // precedents
  useEffect(() => {
    setExplorerGamesPage(0);
    handleClosePreview();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [currentNodeId, explorerFilters]);

  // Meme raison/mecanique que l'effet opening_explorer plus haut (voir
  // useLatestFetch) : un coup par requete rendait aussi cette liste a la
  // traine en jouant vite.
  useLatestFetch(
    !!tree && explorerVisible,
    [tabId, tree, currentNodeId, explorerVisible, explorerGamesPage, explorerGamesSort, explorerFilters],
    () => {
      setExplorerGamesLoading(true);
      const moves = pathSans(tree!, currentNodeId);
      return invoke<ExplorerGamesPage>("opening_explorer_games", {
        tabId,
        moves,
        startFen: rootFen(tree!),
        offset: explorerGamesPage * EXPLORER_GAMES_PAGE_SIZE,
        limit: EXPLORER_GAMES_PAGE_SIZE,
        sortBy: explorerGamesSort?.column,
        sortDir: explorerGamesSort?.dir,
        ...explorerFilterArgs(explorerFilters),
      });
    },
    (page) => {
      setExplorerGames(page.games);
      setExplorerGamesTotal(page.total);
      setExplorerGamesBasePath(page.basePath);
      setExplorerGamesError(null);
      setExplorerGamesLoading(false);
    },
    (e) => {
      setExplorerGamesError(String(e));
      setExplorerGamesLoading(false);
    }
  );

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
        const moves = tree ? pathSans(tree, currentNodeId) : [];
        const { tree: previewedTree, matchNodeId } = await invoke<{ tree: TreeDto; matchNodeId: number | null }>(
          "opening_explorer_preview_game",
          { id, moves, startFen: tree ? rootFen(tree) : undefined }
        );
        setPreviewTree(previewedTree);
        // demarre l'apercu a la position recherchee (celle qui a amene cette
        // partie dans la liste), retrouvee cote backend par sa position
        // (transpositions et departs personnalises compris, voir
        // explorer::mainline_node_at_zobrist) -- pas en comptant des demi-coups
        // depuis l'onglet d'origine, qui ne correspondent a rien si cette
        // partie a atteint la position par un ordre de coups different.
        setPreviewNodeId(matchNodeId ?? previewedTree.root);
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
      const moves = tree ? pathSans(tree, currentNodeId) : [];
      const tab = await invoke<TabDto>("open_game", {
        path: explorerGamesBasePath,
        id: previewGameId,
        moves,
        startFen: tree ? rootFen(tree) : undefined,
      });
      await emit("game-tab-opened", tab);
    } catch (e) {
      setPreviewError(String(e));
    }
  }, [previewGameId, explorerGamesBasePath, tree, currentNodeId]);

  // --- Menus natifs : n'agissent que sur l'onglet actif ---
  useEffect(() => {
    if (!active) return;
    const unlistenPromises = [
      listen("menu-export-pgn", () => handleExport()),
      listen("menu-save-game", () => handleSave()),
      listen<string>("menu-set-board-theme", (event) => setBoardTheme(event.payload)),
      listen("menu-toggle-panel-board", () => toggleGamePanel("board")),
      listen("menu-toggle-panel-notation", () => toggleGamePanel("notation")),
      listen("menu-toggle-panel-explorer", () => toggleGamePanel("explorer")),
      listen("menu-toggle-panel-engine", () => toggleGamePanel("engine")),
      listen("menu-toggle-panel-training", () => toggleGamePanel("training")),
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

      // Voir HomeWindow.tsx : les accelerateurs natifs du menu (Ctrl+S,
      // Ctrl+E) ne se declenchent jamais au clavier sous Windows -- implemente
      // nous-memes.
      if (e.ctrlKey || e.metaKey) {
        const key = e.key.toLowerCase();
        if (key === "s") {
          e.preventDefault();
          handleSave();
          return;
        } else if (key === "e") {
          e.preventDefault();
          handleExport();
          return;
        }
      }

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

      if (e.key === "ArrowLeft") {
        e.preventDefault();
        handleStep("prev");
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        handleStep("next");
      }
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [active, tree, currentNodeId, variationChoice, highlightedIndex, chooseVariation, handleStep, handleSave, handleExport]);

  const onDockviewReady = useCallback(
    (event: DockviewReadyEvent) => {
      dockviewApiRef.current = event.api;
      const api = event.api;
      // explorerVisible ne veut dire "l'onglet Explorateur est au premier
      // plan" (celui reellement affiche a l'ecran), pas juste "il existe
      // quelque part dans la disposition" (getPanel) : sur une base de
      // plusieurs millions de parties, laisser tourner opening_explorer/
      // opening_explorer_games a chaque coup pour un onglet qui existe mais
      // qu'on ne regarde pas (partage un groupe avec Notation, par exemple)
      // coute cher pour rien (rapporte par l'utilisateur). onDidActivePanelChange
      // couvre aussi bien changer d'onglet dans le meme groupe que fermer
      // l'onglet actif (quel que soit le chemin -- bouton "x" ou menu) : la
      // fermeture via son propre bouton ne passait auparavant par aucun code
      // a nous, laissant explorerVisible bloque a true indefiniment.
      const explorerActiveDisposable = api.onDidActivePanelChange(() => setExplorerVisible(api.activePanel?.id === "explorer"));
      explorerVisibilitySyncCleanupRef.current = () => explorerActiveDisposable.dispose();
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
          // une disposition sauvegardee remplace entierement les panneaux
          // (fromJSON) -- si l'echiquier avait ete ferme avant la sauvegarde
          // (fermeture accidentelle de son onglet dockview, qui n'a pas de
          // case a cocher a l'epoque), il resterait sinon introuvable pour
          // toujours : on le regarantit toujours present.
          if (!api.getPanel("board")) setPanelVisible("board", true);
          setExplorerVisible(api.activePanel?.id === "explorer");
        }
        syncPanelCheckedStates(api, GAME_PANEL_IDS);
        gameLayoutCleanupRef.current = persistDockviewLayout(api, GAME_LAYOUT_KEY_PREP, GAME_PANEL_IDS);
        return;
      }
      if (tryRestoreDockviewLayout(api, GAME_LAYOUT_KEY)) {
        if (!api.getPanel("board")) setPanelVisible("board", true);
        setExplorerVisible(api.activePanel?.id === "explorer");
        syncPanelCheckedStates(api, GAME_PANEL_IDS);
        gameLayoutCleanupRef.current = persistDockviewLayout(api, GAME_LAYOUT_KEY, GAME_PANEL_IDS);
        return;
      }
      const prefs = loadGamePanelPrefs();
      if (prefs.notation) setPanelVisible("notation", true);
      if (prefs.explorer) setPanelVisible("explorer", true);
      if (prefs.engine) setPanelVisible("engine", true);
      if (prefs.training) setPanelVisible("training", true);
      api.getPanel("notation")?.api.setActive();
      setExplorerVisible(api.activePanel?.id === "explorer");
      syncPanelCheckedStates(api, GAME_PANEL_IDS);
      gameLayoutCleanupRef.current = persistDockviewLayout(api, GAME_LAYOUT_KEY, GAME_PANEL_IDS);
    },
    [isPlayerPrep, setPanelVisible, t]
  );

  useEffect(() => () => gameLayoutCleanupRef.current?.(), []);
  useEffect(() => () => explorerVisibilitySyncCleanupRef.current?.(), []);

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
    api.getPanel("training")?.api.setTitle(t("gameTab.panel.training"));
  }, [t]);

  const contextValue: AppContextValue = {
    tree,
    currentNode: currentNode ?? null,
    currentNodeId,
    dests,
    boardTheme,
    setBoardTheme,
    boardFlipped,
    setBoardFlipped,
    handleMove,
    handlePlayPvLine,
    handleShapesChange,
    handleSelect,
    onStep: handleStep,
    variationChoice,
    highlightedIndex,
    chooseVariation,
    handleSetComment,
    handleToggleNag,
    handlePromoteVariation,
    handleDeleteVariation,
    handleNewChess960,
    explorerStats,
    explorerStatsLoading,
    explorerError,
    checkSuffixesCleanupCount,
    ecoInfo,
    explorerFilters: explorerFiltersInput,
    onExplorerFiltersChange: setExplorerFiltersInput,
    onExplorerFiltersCommit: commitExplorerFilters,
    handleExplorerMove,
    explorerGames,
    explorerGamesTotal,
    explorerGamesPage,
    explorerGamesSort,
    onExplorerGamesSortChange,
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
    gameReportRunning,
    gameReportProgress,
    gameReportError,
    gameReportSummary,
    handleStartGameReport,
    trainingNodeId,
    trainingSide,
    trainingFen,
    trainingDests,
    trainingFeedback,
    trainingBusy,
    trainingError,
    trainingAtEnd,
    trainingAccuracy,
    trainingSummary,
    trainingDepth,
    setTrainingDepth,
    trainingThreadsSetting,
    setTrainingThreadsSetting,
    handleStartTraining,
    handleStopTraining,
    handleTrainingMove,
    handleTrainingContinue,
  };

  return (
    <AppContext.Provider value={contextValue}>
      <main className="app-layout">
        {statusMessage && <p className="status-message">{statusMessage}</p>}
        {error && <p className="error">{error}</p>}
        <div className={"dockview-container " + theme.className}>
          <DockviewReact components={dockviewComponents} onReady={onDockviewReady} theme={theme} />
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
