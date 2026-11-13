export interface MoveDto {
  from: string;
  to: string;
  promotion: string | null;
  san: string;
}

export interface ShapeDto {
  orig: string;
  dest: string | null;
  brush: string;
}

export interface NodeDto {
  id: number;
  parent: number | null;
  children: number[];
  san: string | null;
  from: string | null;
  to: string | null;
  promotion: string | null;
  fen: string;
  turn: "white" | "black";
  commentBefore: string | null;
  commentAfter: string | null;
  nags: number[];
  shapes: ShapeDto[];
}

export interface TreeDto {
  nodes: NodeDto[];
  root: number;
  tags: Record<string, string>;
}

// Un autre onglet ouvert dans la meme fenetre Partie -- voir App.tsx
// (GameTabInfo) et Notation.tsx/GameLinkPicker.tsx (citation d'une partie
// dans un commentaire, restreinte aux onglets deja ouverts).
export interface OpenTabRef {
  id: number;
  title: string;
}

export interface PvMove {
  san: string;
  fen: string;
  from: string;
  to: string;
  promotion: string | null;
}

export interface EngineInfo {
  depth: number;
  multipv: number;
  scoreCp: number | null;
  scoreMate: number | null;
  pv: PvMove[];
  /** gain / nulle / perte pour mille, du point de vue des Blancs -- null si
      le moteur ne fournit pas ces probabilites (voir engine.rs). */
  wdl: [number, number, number] | null;
  /** statistiques de recherche (ligne d'etat du panneau Moteur), chacune
      null si le moteur ne l'envoie pas ; hashfull est en pour mille. */
  seldepth: number | null;
  nodes: number | null;
  nps: number | null;
  hashfull: number | null;
}

export interface PositionEval {
  scoreCp: number | null;
  scoreMate: number | null;
}

// Comparaison DIRECTE du coup essaye au coup REELLEMENT joue dans la partie
// (independante de classification, qui compare plutot au meilleur coup
// trouve par le moteur) -- "similar" couvre une difference negligeable
// (bruit de recherche) ou une veritable egalite d'evaluation.
export type MoveComparison = "better" | "worse" | "similar";

export interface TrainingPrefetch {
  nodeId: number;
  evalBefore: PositionEval;
  evalGame: PositionEval;
  // SAN du coup que le moteur juge meilleur que le coup de la partie
  // lui-meme (independant du coup essaye) ; null si le coup de la partie
  // etait deja le meilleur trouve, ou pas clairement pire.
  betterGameMoveSan: string | null;
  gameMoveClassification: number | null;
}

export interface TrainingJudgement {
  matchesGame: boolean;
  gameMoveComparison: MoveComparison;
  // code NAG (2/4/6, voir nag.ts) si le coup essaye est une
  // imprecision/erreur/gaffe par rapport a la position de depart ; null sinon.
  classification: number | null;
  refutation: PvMove[];
  accuracy: number;
  betterGameMoveSan: string | null;
  gameMoveClassification: number | null;
}

export interface GameReportProgress {
  done: number;
  total: number;
}

export interface GameReportMoveEval {
  nodeId: number;
  ply: number;
  san: string | null;
  whiteWinPercent: number;
  scoreCp: number | null;
  scoreMate: number | null;
  classification: number | null;
}

export interface GameReportSideStats {
  accuracy: number;
  blunders: number;
  mistakes: number;
  inaccuracies: number;
  avgCentipawnLoss: number;
}

export interface GameReportSummary {
  white: GameReportSideStats;
  black: GameReportSideStats;
  moves: GameReportMoveEval[];
}

export interface PgnGameSummary {
  index: number;
  white: string;
  black: string;
  date: string;
  result: string;
  event: string;
  whiteElo: number | null;
  blackElo: number | null;
}

export interface OpeningMoveStat {
  san: string;
  games: number;
  whiteWins: number;
  draws: number;
  blackWins: number;
}

export interface EcoInfo {
  eco: string;
  name: string;
}

export interface ExplorerFilterValues {
  playerName: string;
  playerWhite: boolean;
  ratingMin: string;
  ratingMax: string;
  yearMin: string;
  yearMax: string;
}

export interface TournamentSummary {
  id: number;
  name: string;
  site: string;
  timeControl: string | null;
  games: number;
  dateStart: string | null;
  dateEnd: string | null;
}

export interface BaseInfo {
  name: string;
  path: string;
  games: number;
  isReference: boolean;
  missing: boolean;
}

// Megabase telechargeable (voir megabase.rs)
export interface MegabaseManifest {
  version: string;
  url: string;
  compressedSize: number;
  size: number;
  sha256: string;
  games: number;
  countUrl?: string | null;
}

export interface MegabasePending {
  dir: string;
  downloaded: number;
  manifest: MegabaseManifest;
}

export interface MegabaseProgress {
  stage: "download" | "verify" | "decompress";
  done: number;
  total: number;
  bytesPerSec: number;
}

export interface MegabaseError {
  code: string;
  detail: string | null;
  needed: number | null;
  available: number | null;
}

export interface GameSummary {
  id: number;
  white: string;
  black: string;
  date: string;
  result: string;
  event: string;
  timeControl: string;
  whiteElo: number | null;
  blackElo: number | null;
}

export type SaveResult = { kind: "saved"; id: number } | { kind: "duplicate"; existing: GameSummary };

export type ExplorerGameSummary = GameSummary;

export type ExplorerGamesSortColumn = "white" | "black" | "date" | "whiteElo" | "blackElo" | "result";
export interface ExplorerGamesSort {
  column: ExplorerGamesSortColumn;
  dir: "asc" | "desc";
}

export interface ExplorerGamesPage {
  games: ExplorerGameSummary[];
  total: number;
  basePath: string;
}

export interface PlayerSummary {
  name: string;
  games: number;
}

export interface PlayersPage {
  players: PlayerSummary[];
  total: number;
}

export interface TournamentsPage {
  tournaments: TournamentSummary[];
  total: number;
}

export interface FidePlayer {
  fideId: string;
  name: string;
  fed: string;
  sex: string;
  title: string;
  wTitle: string;
  oTitle: string;
  stdRating: number | null;
  rapidRating: number | null;
  blitzRating: number | null;
  birthYear: number | null;
  flag: string;
}

export interface FidePlayersPage {
  players: FidePlayer[];
  total: number;
}

export interface FideStatus {
  lastUpdated: string | null;
  total: number;
}

export interface TabDto {
  tabId: number;
  tree: TreeDto;
  isPlayerPrep: boolean;
  initialNodeId: number | null;
}

export interface BulkImportSummary {
  total: number;
  imported: number;
  skippedDuplicates: number;
  skippedInvalid: number;
  failed: number;
}

export interface CatchupRange {
  fromIssue: number;
  toIssue: number;
  weeksBehind: number;
}

export interface UpdatePreviewTournament {
  event: string;
  games: number;
  /** "twic" | "lichess" | "both" */
  source: string;
  dateStart: string | null;
  dateEnd: string | null;
}

export interface UpdatePreview {
  tournaments: UpdatePreviewTournament[];
  totalGames: number;
  fromIssue: number;
  toIssue: number;
  weeksBehind: number;
  pgnPath: string;
}
