import { createContext, useContext } from "react";
import type { Dests, Key } from "@lichess-org/chessground/types";
import type {
  EcoInfo,
  EngineInfo,
  ExplorerFilterValues,
  ExplorerGameSummary,
  ExplorerGamesSort,
  ExplorerGamesSortColumn,
  GameReportProgress,
  GameReportSummary,
  MoveComparison,
  NodeDto,
  OpenTabRef,
  OpeningMoveStat,
  PvMove,
  ShapeDto,
  TreeDto,
} from "./types";

export interface TrainingFeedback {
  matchesGame: boolean;
  gameSan: string;
  classification: number | null;
  gameMoveComparison: MoveComparison;
  refutation: PvMove[];
  accuracy: number;
  // SAN du coup que le moteur juge meilleur que le coup de la partie
  // lui-meme (independant du coup essaye) -- affiche meme quand matchesGame
  // est vrai : rejouer le coup de la partie ne garantit pas que c'etait le
  // meilleur coup.
  betterGameMoveSan: string | null;
  gameMoveClassification: number | null;
  // noeud reel de la partie vers lequel avancer une fois le retour affiche
  // (voir handleTrainingContinue, GameTab.tsx) ; absent quand matchesGame est
  // deja vrai (l'avancee est alors automatique, pas de bouton "Continuer").
  pendingNodeId: number | null;
}

// Resume d'une session d'entrainement terminee (fin de la ligne atteinte) --
// memes champs que GameReportSideStats (rapport de partie) pour reutiliser
// telles quelles les classes CSS .game-report-* (voir TrainingPanel.tsx),
// mais calcule uniquement a partir des coups REELLEMENT essayes pendant la
// session, pas d'une analyse complete de la partie.
export interface TrainingSessionSummary {
  accuracy: number;
  blunders: number;
  mistakes: number;
  inaccuracies: number;
  rounds: number;
}

export interface AppContextValue {
  tree: TreeDto | null;
  currentNode: NodeDto | null;
  currentNodeId: number;
  dests: Dests;
  boardTheme: string;
  setBoardTheme: (theme: string) => void;
  boardFlipped: boolean;
  setBoardFlipped: (flipped: boolean) => void;
  handleMove: (from: Key, to: Key, promotion?: string | null) => void;
  handlePlayPvLine: (moves: PvMove[], uptoIndex: number) => void;
  handleShapesChange: (shapes: ShapeDto[]) => void;
  handleSelect: (id: number) => void;
  onStep: (direction: "start" | "prev" | "next" | "end") => void;
  variationChoice: NodeDto[] | null;
  highlightedIndex: number;
  chooseVariation: (id: number) => void;
  handleSetComment: (before: string, after: string) => void;
  // Clic sur une citation de partie ("[%game ...]") dans un commentaire,
  // voir Notation.tsx -- ouvre la partie visee (n'importe quelle base,
  // pas forcement celle de l'onglet courant) comme nouvel onglet.
  handleOpenGameLink: (path: string, id: number) => void;
  // Autres onglets ouverts dans cette fenetre (voir App.tsx), proposes par
  // GameLinkPicker comme seules cibles possibles pour une citation.
  otherTabs: OpenTabRef[];
  handleToggleNag: (code: number) => void;
  handlePromoteVariation: () => void;
  handleDeleteVariation: () => void;
  handleNewChess960: (positionNumber: number) => void;
  explorerStats: OpeningMoveStat[];
  explorerStatsLoading: boolean;
  explorerError: string | null;
  checkSuffixesCleanupCount: number | null;
  ecoInfo: EcoInfo | null;
  explorerFilters: ExplorerFilterValues;
  onExplorerFiltersChange: (filters: ExplorerFilterValues) => void;
  onExplorerFiltersCommit: () => void;
  handleExplorerMove: (san: string) => void;

  explorerGames: ExplorerGameSummary[];
  explorerGamesTotal: number;
  explorerGamesPage: number;
  explorerGamesSort: ExplorerGamesSort | null;
  onExplorerGamesSortChange: (column: ExplorerGamesSortColumn) => void;
  explorerGamesLoading: boolean;
  explorerGamesError: string | null;
  onExplorerGamesPageChange: (page: number) => void;

  previewGameId: number | null;
  previewTree: TreeDto | null;
  previewNodeId: number | null;
  previewError: string | null;
  onSelectPreviewGame: (id: number) => void;
  onSelectPreviewNode: (id: number) => void;
  onPreviewStep: (direction: "start" | "prev" | "next" | "end") => void;
  onOpenPreviewGame: () => void;
  onClosePreview: () => void;

  enginePath: string;
  setEnginePath: (path: string) => void;
  engineRunning: boolean;
  engineStarting: boolean;
  enginePaused: boolean;
  engineDepth: number;
  setEngineDepth: (depth: number) => void;
  engineMultiPv: number;
  setEngineMultiPv: (multiPv: number) => void;
  engineThreads: number;
  setEngineThreads: (threads: number) => void;
  // table de hachage du moteur, en Mo
  engineHash: number;
  setEngineHash: (hashMb: number) => void;
  // plafond (75 % de la memoire du PC), null s'il est inconnu
  maxEngineHash: number | null;
  maxThreads: number;
  engineLines: EngineInfo[];
  engineError: string | null;
  handleStartEngine: () => void;
  handleStopEngine: () => void;
  handlePauseEngine: () => void;
  handleResumeEngine: () => void;

  gameReportRunning: boolean;
  gameReportProgress: GameReportProgress | null;
  gameReportError: string | null;
  gameReportSummary: GameReportSummary | null;
  handleStartGameReport: (depth: number, threads: number) => void;

  trainingNodeId: number | null;
  trainingSide: "white" | "black" | null;
  trainingFen: string | null;
  // l'utilisateur a quitte la position de l'exercice (navigation libre sur
  // l'echiquier principal, partage avec l'exercice) : voir GameTab.tsx.
  trainingAway: boolean;
  // l'echiquier principal sert en ce moment a l'exercice : le panneau Moteur
  // masque alors son analyse (elle donnerait le coup a trouver).
  trainingOnBoard: boolean;
  // premier coup de la suite de la partie a masquer dans la Notation
  // pendant l'exercice (null hors exercice), voir GameTab.tsx.
  trainingHiddenFromId: number | null;
  trainingFeedback: TrainingFeedback | null;
  trainingBusy: boolean;
  trainingError: string | null;
  trainingAtEnd: boolean;
  // precision cumulee de la session en cours (moyenne des coups deja juges,
  // voir trainingRounds dans GameTab.tsx) -- null tant qu'aucun coup n'a
  // encore ete juge, pour distinguer "pas encore de score" de "0%".
  trainingAccuracy: number | null;
  trainingSummary: TrainingSessionSummary | null;
  // Reglage moteur PROPRE a l'entrainement (pas celui du rapport de partie) :
  // visible/reglable directement dans TrainingPanel, voir son usage dans
  // GameTab.tsx pour pourquoi un reglage invisible ici ne suffisait pas.
  trainingDepth: number;
  setTrainingDepth: (depth: number) => void;
  trainingThreadsSetting: number;
  setTrainingThreadsSetting: (threads: number) => void;
  handleStartTraining: (nodeId: number) => void;
  handleStopTraining: () => void;
  handleTrainingResume: () => void;
  handleTrainingContinue: () => void;
}

export const AppContext = createContext<AppContextValue | null>(null);

export function useAppContext(): AppContextValue {
  const ctx = useContext(AppContext);
  if (!ctx) {
    throw new Error("useAppContext doit etre utilise a l'interieur de AppContext.Provider");
  }
  return ctx;
}
