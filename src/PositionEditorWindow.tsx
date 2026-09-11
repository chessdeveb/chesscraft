import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import type { Color as PieceColor, Key, Piece, Role } from "@lichess-org/chessground/types";
import { PositionEditorBoard, type PositionEditorBoardHandle } from "./components/PositionEditorBoard";
import { loadStoredBoardTheme } from "./boardTheme";
import { useSyncLanguage } from "./i18n";
import { useSyncColorScheme } from "./colorScheme";
import { useAutoDismissError } from "./useAutoDismissError";
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

function parseCastlingField(s: string): CastlingRights {
  return { wk: s.includes("K"), wq: s.includes("Q"), bk: s.includes("k"), bq: s.includes("q") };
}

// La rangee de la case de prise en passant decoule entierement du trait : le
// camp au trait est celui qui capturerait, donc c'est forcement l'AUTRE
// camp qui vient de jouer le double-pas -- rangee 6 (survolee par un pion
// noir) si les Blancs ont le trait, rangee 3 (pion blanc) si ce sont les
// Noirs. Ex. "1. e4" donne le FEN "... b ... e3 ..." : Noirs au trait,
// rangee 3.
function enPassantSquare(file: string, sideToMove: "w" | "b"): string {
  if (!file) return "-";
  const rank = sideToMove === "w" ? "6" : "3";
  return `${file}${rank}`;
}

// Inverse d'enPassantSquare : ne garde que la colonne d'une case FEN
// ("e3" -> "e"), la rangee etant redondante avec le trait (voir plus haut).
function enPassantFileFromSquare(square: string): string {
  return /^[a-h][1-8]$/.test(square) ? square[0] : "";
}

export function PositionEditorWindow() {
  useSyncLanguage();
  useSyncColorScheme();
  const { t } = useTranslation();
  const [boardTheme] = useState(loadStoredBoardTheme);
  const boardRef = useRef<PositionEditorBoardHandle>(null);
  const [sideToMove, setSideToMove] = useState<"w" | "b">("w");
  const [castling, setCastling] = useState<CastlingRights>({ wk: true, wq: true, bk: true, bq: true });
  // Colonne ("a" a "h") du pion qui vient de faire un double-pas, ou "" si
  // aucune prise en passant n'est possible dans cette position. La rangee,
  // elle, n'a pas besoin d'etre saisie : elle decoule entierement du trait
  // (case survolee par un double-pas noir si les Blancs ont le trait -- 6e
  // rangee -- ou blanc si les Noirs l'ont -- 3e rangee), voir enPassantSquare.
  const [enPassantFile, setEnPassantFile] = useState("");
  const [error, setError] = useAutoDismissError();
  // piece "armee" depuis la palette : reste selectionnee apres la pose pour
  // pouvoir cliquer plusieurs cases de suite sans repasser par la palette a
  // chaque fois -- seul un clic sur la palette (meme piece pour desarmer,
  // autre piece pour changer) y met fin.
  const [selectedPiece, setSelectedPiece] = useState<Piece | null>(null);
  const [fenInput, setFenInput] = useState("");

  // Petite fenetre utilitaire : pas de menu natif souhaite (voir
  // lib.rs::remove_window_menu -- sous Windows, une fenetre creee sans menu
  // explicite peut malgre tout heriter du menu partage de l'appli, de facon
  // intermittente).
  useEffect(() => {
    invoke("remove_window_menu").catch(() => {});
  }, []);

  // Applique un FEN colle : seul le champ "placement des pieces" doit avoir
  // une forme plausible (8 groupes separes par '/') pour etre accepte ici --
  // la validation complete (position legale, roi manquant...) reste faite
  // cote backend a la validation finale (voir GameTree::from_fen), inutile
  // de la dupliquer pour un simple chargement dans l'editeur.
  function applyFen(fen: string) {
    const parts = fen.trim().split(/\s+/);
    const boardPart = parts[0] ?? "";
    if (boardPart.split("/").length !== 8) {
      setError(t("positionEditor.fenInvalid"));
      return;
    }
    boardRef.current?.setBoardFen(boardPart);
    setSideToMove(parts[1] === "b" ? "b" : "w");
    setCastling(parts[2] ? parseCastlingField(parts[2]) : { wk: false, wq: false, bk: false, bq: false });
    setEnPassantFile(parts[3] ? enPassantFileFromSquare(parts[3]) : "");
    setSelectedPiece(null);
    setError(null);
  }

  function handleLoadFen() {
    if (!fenInput.trim()) return;
    applyFen(fenInput);
  }

  function handleFenPaste(e: React.ClipboardEvent<HTMLInputElement>) {
    const pasted = e.clipboardData.getData("text");
    if (!pasted.trim()) return;
    e.preventDefault();
    setFenInput(pasted);
    applyFen(pasted);
  }

  function handlePaletteClick(role: Role, color: PieceColor) {
    return () => {
      setSelectedPiece((prev) => (prev && prev.role === role && prev.color === color ? null : { role, color }));
    };
  }

  function handleSquareClick(key: Key) {
    if (!selectedPiece) return;
    boardRef.current?.togglePiece(selectedPiece, key);
  }

  // capture e.currentTarget.checked synchronement, avant l'appel a
  // setCastling : React remet currentTarget a null une fois l'evenement
  // traite, et l'updater fonctionnel passe a setCastling peut s'executer
  // plus tard (StrictMode le rejoue notamment) -- le lire a l'interieur de
  // l'updater plantait alors sur `null.checked`, systematiquement en
  // recochant une case apres un changement de position (Echiquier vide...).
  function handleCastlingChange(field: keyof CastlingRights) {
    return (e: React.ChangeEvent<HTMLInputElement>) => {
      const checked = e.currentTarget.checked;
      setCastling((c) => ({ ...c, [field]: checked }));
    };
  }

  function handleReset() {
    boardRef.current?.setBoardFen(STANDARD_BOARD_FEN);
    setSideToMove("w");
    setCastling({ wk: true, wq: true, bk: true, bq: true });
    setEnPassantFile("");
  }

  function handleClear() {
    boardRef.current?.setBoardFen(EMPTY_BOARD_FEN);
    setCastling({ wk: false, wq: false, bk: false, bq: false });
    setEnPassantFile("");
  }

  function handleEnPassantFileChange(e: React.ChangeEvent<HTMLInputElement>) {
    const c = e.target.value.trim().toLowerCase().slice(-1);
    setEnPassantFile(/^[a-h]$/.test(c) ? c : "");
  }

  async function handleCancel() {
    await getCurrentWindow().close();
  }

  async function handleConfirm() {
    const boardFen = boardRef.current?.getBoardFen();
    if (!boardFen) return;
    const fen = `${boardFen} ${sideToMove} ${castlingField(castling)} ${enPassantSquare(enPassantFile, sideToMove)} 0 1`;
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
      <div className="position-editor-fen-row">
        <input
          type="text"
          value={fenInput}
          onChange={(e) => setFenInput(e.target.value)}
          onPaste={handleFenPaste}
          onKeyDown={(e) => {
            if (e.key === "Enter") handleLoadFen();
          }}
          placeholder={t("positionEditor.fenPlaceholder")}
        />
        <button onClick={handleLoadFen}>{t("positionEditor.fenLoad")}</button>
      </div>
      <div className="position-editor-body">
        <div className="position-editor-board-col">
          <PositionEditorBoard ref={boardRef} theme={boardTheme} onSquareClick={handleSquareClick} />
          <div className="position-editor-palette">
            {(["white", "black"] as PieceColor[]).map((color) => (
              <div className="position-editor-palette-row" key={color}>
                {PALETTE_ROLES.map((role) => (
                  <div
                    key={role}
                    className={
                      "cg-wrap position-editor-swatch" +
                      (selectedPiece?.role === role && selectedPiece?.color === color ? " position-editor-swatch--selected" : "")
                    }
                    title={t(`positionEditor.piece.${role}`)}
                    onClick={handlePaletteClick(role, color)}
                  >
                    <piece className={`${role} ${color}`} />
                  </div>
                ))}
              </div>
            ))}
          </div>
          <p className="position-editor-hint">{t("positionEditor.paletteHint")}</p>
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
                onChange={handleCastlingChange("wk")}
              />
              {t("positionEditor.castlingWhiteKingside")}
            </label>
            <label>
              <input
                type="checkbox"
                checked={castling.wq}
                onChange={handleCastlingChange("wq")}
              />
              {t("positionEditor.castlingWhiteQueenside")}
            </label>
            <label>
              <input
                type="checkbox"
                checked={castling.bk}
                onChange={handleCastlingChange("bk")}
              />
              {t("positionEditor.castlingBlackKingside")}
            </label>
            <label>
              <input
                type="checkbox"
                checked={castling.bq}
                onChange={handleCastlingChange("bq")}
              />
              {t("positionEditor.castlingBlackQueenside")}
            </label>
          </div>
          <div className="position-editor-section">
            <span className="position-editor-section-title">{t("positionEditor.enPassant")}</span>
            <label>
              {t("positionEditor.enPassantColumn")}
              <input
                type="text"
                className="position-editor-en-passant-input"
                maxLength={1}
                value={enPassantFile}
                onChange={handleEnPassantFileChange}
                placeholder="–"
              />
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
