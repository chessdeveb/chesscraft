import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import { Chessground } from "@lichess-org/chessground";
import type { Api } from "@lichess-org/chessground/api";
import type { Key, Piece } from "@lichess-org/chessground/types";

export interface PositionEditorBoardHandle {
  /** Le champ "placement des pieces" du FEN (rangs 8->1), sans trait/roques/etc. */
  getBoardFen: () => string;
  setBoardFen: (fen: string) => void;
  /** A appeler depuis le mousedown/touchstart d'une piece de la palette. */
  dragNewPiece: (piece: Piece, event: MouseEvent | TouchEvent) => void;
  /** Pose la piece armee sur la case cliquee -- si la case porte deja
      exactement cette piece, la retire au lieu de la reposer (bascule),
      sinon la remplace (case vide ou piece differente). */
  togglePiece: (piece: Piece, key: Key) => void;
}

interface PositionEditorBoardProps {
  theme: string;
  /** Case cliquee sur le plateau -- la fenetre parente decide quoi en faire
      (poser la piece armee, ou rien si aucune n'est selectionnee). */
  onSquareClick?: (key: Key) => void;
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
  function PositionEditorBoard({ theme, onSquareClick }, ref) {
    const elRef = useRef<HTMLDivElement>(null);
    const apiRef = useRef<Api | null>(null);
    const onSquareClickRef = useRef(onSquareClick);
    onSquareClickRef.current = onSquareClick;

    useEffect(() => {
      if (!elRef.current) return;
      apiRef.current = Chessground(elRef.current, {
        coordinates: true,
        ranksPosition: "right",
        movable: { free: true, color: "both" },
        draggable: { deleteOnDropOff: true },
        drawable: { enabled: false },
        highlight: { lastMove: false },
        events: { select: (key) => onSquareClickRef.current?.(key) },
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
      togglePiece: (piece, key) => {
        const api = apiRef.current;
        if (!api) return;
        const existing = api.state.pieces.get(key);
        const same = existing && existing.role === piece.role && existing.color === piece.color;
        // setPieces dans les deux cas, jamais newPiece : celui-ci (via
        // board.baseNewPiece) refuse silencieusement de poser sur une case
        // deja occupee sans l'option force -- le remplacement (piece
        // differente sur une case occupee) ne faisait donc rien du tout.
        api.setPieces(new Map([[key, same ? undefined : piece]]));
        // sinon la case reste visuellement "selectionnee" (surbrillance
        // chessground), comme si on s'appretait a y jouer un coup.
        api.selectSquare(null);
      },
    }));

    return (
      // Pas .board-wrap (100%/100% + centrage) : concu pour le plateau de
      // jeu, qui remplit tout un panneau dockview redimensionnable. Ici la
      // taille est fixe (320x320) dans une colonne flex qui contient aussi
      // la palette -- .board-wrap s'etirait alors sur toute la hauteur
      // disponible, ecrasant/deplacant la palette en dessous plutot que de
      // se contenter de sa taille naturelle.
      <div className={`board-theme-${theme}`}>
        <div ref={elRef} className="cg-wrap position-editor-cg-wrap" />
      </div>
    );
  }
);
