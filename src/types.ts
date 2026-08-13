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
}

export interface EngineInfo {
  depth: number;
  multipv: number;
  scoreCp: number | null;
  scoreMate: number | null;
  pvSan: string[];
}

export interface PgnGameSummary {
  index: number;
  white: string;
  black: string;
  date: string;
  result: string;
  event: string;
}

export interface OpeningMoveStat {
  san: string;
  games: number;
  whiteWins: number;
  draws: number;
  blackWins: number;
}
