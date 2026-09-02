import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { Chessground } from "@lichess-org/chessground";
import type { Api } from "@lichess-org/chessground/api";
import type { Piece } from "@lichess-org/chessground/types";

export interface PositionEditorBoardHandle {
  /** Le champ "placement des pieces" du FEN (rangs 8->1), sans trait/roques/etc. */
  getBoardFen: () => string;
  setBoardFen: (fen: string) => void;
  /** A appeler depuis le mousedown/touchstart d'une piece de la palette. */
  dragNewPiece: (piece: Piece, event: MouseEvent | TouchEvent) => void;
}

interface PositionEditorBoardProps {
  theme: string;
}

// Echiquier de l'editeur de position (Fichier > Nouvelle position) : distinct
// du Board de jeu normal, expres -- ici les pieces se deplacent librement
// (movable.free), sans notion de coup legal/tour, et la piece deposee hors
// du plateau doit disparaitre (comportement natif de chessground en mode
// free, pas besoin de le coder). Pas de props fen/dests/onMove reactives
// comme le Board de jeu : l'etat vit entierement dans chessground lui-meme,
// piloté imperativement par la fenetre parente via ce handle (palette de
// pieces, boutons position de depart/vide).
export const PositionEditorBoard = forwardRef<PositionEditorBoardHandle, PositionEditorBoardProps>(
  function PositionEditorBoard({ theme }, ref) {
    const elRef = useRef<HTMLDivElement>(null);
    const apiRef = useRef<Api | null>(null);

    useEffect(() => {
      if (!elRef.current) return;
      apiRef.current = Chessground(elRef.current, {
        coordinates: true,
        ranksPosition: "right",
        movable: { free: true, color: "both" },
        draggable: { deleteOnDropOff: true },
        drawable: { enabled: false },
        highlight: { lastMove: false },
      });
      return () => {
        apiRef.current?.destroy();
        apiRef.current = null;
      };
    }, []);

    useImperativeHandle(ref, () => ({
      getBoardFen: () => apiRef.current?.getFen() ?? "8/8/8/8/8/8/8/8",
      setBoardFen: (fen: string) => apiRef.current?.set({ fen }),
      dragNewPiece: (piece, event) => apiRef.current?.dragNewPiece(piece, event),
    }));

    return (
      <div className={`board-wrap board-theme-${theme}`}>
        <div ref={elRef} className="cg-wrap position-editor-cg-wrap" />
      </div>
    );
  }
);
