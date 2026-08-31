export const GAME_DRAG_MIME = "application/x-chesscraft-game-ids";

export interface GameDragPayload {
  ids: number[];
  fromPath: string;
}
