import { useRef, useState } from "react";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import type { Color as PieceColor, Piece, Role } from "@lichess-org/chessground/types";
import { PositionEditorBoard, type PositionEditorBoardHandle } from "./components/PositionEditorBoard";
import { loadStoredBoardTheme } from "./boardTheme";
import { useSyncLanguage } from "./i18n";
import { useSyncColorScheme } from "./colorScheme";
import "@lichess-org/chessground/assets/chessground.base.css";
import "@lichess-org/chessground/assets/chessground.brown.css";
import "@lichess-org/chessground/assets/chessground.cburnett.css";
import "./board-themes.css";
import "./App.css";

const STANDARD_BOARD_FEN = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR";
const EMPTY_BOARD_FEN = "8/8/8/8/8/8/8/8";

const PALETTE_ROLES: Role[] = ["king", "queen", "rook", "bishop", "knight", "pawn"];

interface CastlingRights {
  wk: boolean;
  wq: boolean;
  bk: boolean;
  bq: boolean;
}

function castlingField(rights: CastlingRights): string {
  const s = (rights.wk ? "K" : "") + (rights.wq ? "Q" : "") + (rights.bk ? "k" : "") + (rights.bq ? "q" : "");
  return s || "-";
}

export function PositionEditorWindow() {
  useSyncLanguage();
  useSyncColorScheme();
  const { t } = useTranslation();
  const [boardTheme] = useState(loadStoredBoardTheme);
  const boardRef = useRef<PositionEditorBoardHandle>(null);
  const [sideToMove, setSideToMove] = useState<"w" | "b">("w");
  const [castling, setCastling] = useState<CastlingRights>({ wk: true, wq: true, bk: true, bq: true });
  const [error, setError] = useState<string | null>(null);

  function handlePaletteDown(role: Role, color: PieceColor) {
    return (e: React.MouseEvent | React.TouchEvent) => {
      e.preventDefault();
      const piece: Piece = { role, color };
      boardRef.current?.dragNewPiece(piece, e.nativeEvent as MouseEvent | TouchEvent);
    };
  }

  function handleReset() {
    boardRef.current?.setBoardFen(STANDARD_BOARD_FEN);
    setSideToMove("w");
    setCastling({ wk: true, wq: true, bk: true, bq: true });
  }

  function handleClear() {
    boardRef.current?.setBoardFen(EMPTY_BOARD_FEN);
    setCastling({ wk: false, wq: false, bk: false, bq: false });
  }

  async function handleCancel() {
    await getCurrentWindow().close();
  }

  async function handleConfirm() {
    const boardFen = boardRef.current?.getBoardFen();
    if (!boardFen) return;
    const fen = `${boardFen} ${sideToMove} ${castlingField(castling)} - 0 1`;
    setError(null);
    try {
      await emit("position-editor-confirmed", fen);
      await getCurrentWindow().close();
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <main className="position-editor">
      <h2>{t("positionEditor.title")}</h2>
      {error && <p className="error">{error}</p>}
      <div className="position-editor-body">
        <div className="position-editor-board-col">
          <PositionEditorBoard ref={boardRef} theme={boardTheme} />
          <div className="position-editor-palette">
            {(["white", "black"] as PieceColor[]).map((color) => (
              <div className="position-editor-palette-row" key={color}>
                {PALETTE_ROLES.map((role) => (
                  <div
                    key={role}
                    className="cg-wrap position-editor-swatch"
                    title={t(`positionEditor.piece.${role}`)}
                    onMouseDown={handlePaletteDown(role, color)}
                    onTouchStart={handlePaletteDown(role, color)}
                  >
                    <piece className={`${role} ${color}`} />
                  </div>
                ))}
              </div>
            ))}
          </div>
        </div>
        <div className="position-editor-controls">
          <div className="position-editor-section">
            <span className="position-editor-section-title">{t("positionEditor.sideToMove")}</span>
            <label>
              <input type="radio" checked={sideToMove === "w"} onChange={() => setSideToMove("w")} />
              {t("common.white")}
            </label>
            <label>
              <input type="radio" checked={sideToMove === "b"} onChange={() => setSideToMove("b")} />
              {t("common.black")}
            </label>
          </div>
          <div className="position-editor-section">
            <span className="position-editor-section-title">{t("positionEditor.castling")}</span>
            <label>
              <input
                type="checkbox"
                checked={castling.wk}
                onChange={(e) => setCastling((c) => ({ ...c, wk: e.currentTarget.checked }))}
              />
              {t("positionEditor.castlingWhiteKingside")}
            </label>
            <label>
              <input
                type="checkbox"
                checked={castling.wq}
                onChange={(e) => setCastling((c) => ({ ...c, wq: e.currentTarget.checked }))}
              />
              {t("positionEditor.castlingWhiteQueenside")}
            </label>
            <label>
              <input
                type="checkbox"
                checked={castling.bk}
                onChange={(e) => setCastling((c) => ({ ...c, bk: e.currentTarget.checked }))}
              />
              {t("positionEditor.castlingBlackKingside")}
            </label>
            <label>
              <input
                type="checkbox"
                checked={castling.bq}
                onChange={(e) => setCastling((c) => ({ ...c, bq: e.currentTarget.checked }))}
              />
              {t("positionEditor.castlingBlackQueenside")}
            </label>
          </div>
          <div className="position-editor-section position-editor-reset-buttons">
            <button onClick={handleReset}>{t("positionEditor.standardPosition")}</button>
            <button onClick={handleClear}>{t("positionEditor.emptyBoard")}</button>
          </div>
          <div className="position-editor-actions">
            <button onClick={handleCancel}>{t("common.cancel")}</button>
            <button onClick={handleConfirm}>{t("positionEditor.confirm")}</button>
          </div>
        </div>
      </div>
    </main>
  );
}
