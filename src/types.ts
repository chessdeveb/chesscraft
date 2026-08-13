export interface MoveDto {
  from: string;
  to: string;
  promotion: string | null;
  san: string;
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
}

export interface TreeDto {
  nodes: NodeDto[];
  root: number;
}
