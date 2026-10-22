import { useEffect, useRef, useState } from "react";
import { Chessground } from "@lichess-org/chessground";
import type { Api } from "@lichess-org/chessground/api";
import type { Config } from "@lichess-org/chessground/config";
import type { DrawShape } from "@lichess-org/chessground/draw";
import type { Color, Dests, Key } from "@lichess-org/chessground/types";
import type { ShapeDto } from "../types";

interface BoardProps {
  fen: string;
  turnColor: Color;
  dests: Dests;
  shapes: ShapeDto[];
  theme: string;
  lastMove?: [Key, Key] | null;
  /** Camp affiche en bas de l'echiquier -- "white" si absent. */
  orientation?: Color;
  onMove: (from: Key, to: Key, promotion: string | null) => void;
  onShapesChange: (shapes: ShapeDto[]) => void;
  /** Taille (px, cote du carre) recalculee a chaque redimensionnement --
      pour un element externe (pendules pres de l'echiquier, voir BoardPanel)
      qui doit s'aligner exactement sur la largeur reelle de l'echiquier,
      pas sur celle, plus large, du panneau qui le contient. */
  onSizeChange?: (size: number) => void;
}

function toDrawShapes(shapes: ShapeDto[]): DrawShape[] {
  return shapes.map((s) => ({
    orig: s.orig as Key,
    dest: s.dest ? (s.dest as Key) : undefined,
    brush: s.brush,
  }));
}

function fromDrawShapes(shapes: DrawShape[]): ShapeDto[] {
  return shapes.map((s) => ({
    orig: s.orig,
    dest: s.dest ?? null,
    brush: s.brush ?? "green",
  }));
}

/** Lettre FEN de la piece sur `square` ("P"/"p" pour un pion), ou null si la
    case est vide -- juste assez de lecture de FEN pour detecter une
    promotion (voir le callback "after" plus bas), pas un parseur complet. */
function pieceAt(fen: string, square: string): string | null {
  const rank = Number(square[1]);
  const file = square.charCodeAt(0) - 97;
  const rankStr = fen.split(" ")[0].split("/")[8 - rank];
  let col = 0;
  for (const ch of rankStr) {
    if (ch >= "1" && ch <= "9") {
      col += Number(ch);
    } else {
      if (col === file) return ch;
      col += 1;
    }
  }
  return null;
}

const PROMOTION_ROLES: { role: string; white: string; black: string }[] = [
  { role: "q", white: "♕", black: "♛" },
  { role: "r", white: "♖", black: "♜" },
  { role: "b", white: "♗", black: "♝" },
  { role: "n", white: "♘", black: "♞" },
];

export function Board({ fen, turnColor, dests, shapes, theme, lastMove, orientation, onMove, onShapesChange, onSizeChange }: BoardProps) {
  const outerRef = useRef<HTMLDivElement>(null);
  const elRef = useRef<HTMLDivElement>(null);
  const apiRef = useRef<Api | null>(null);
  const onMoveRef = useRef(onMove);
  onMoveRef.current = onMove;
  const onShapesChangeRef = useRef(onShapesChange);
  onShapesChangeRef.current = onShapesChange;
  const onSizeChangeRef = useRef(onSizeChange);
  onSizeChangeRef.current = onSizeChange;
  const [size, setSize] = useState(480);
  // Lu dans le callback "after" de chessground (fige a la creation, voir
  // l'effet de montage ci-dessous) pour toujours connaitre le FEN juste
  // avant le coup, meme si le prop a change depuis -- meme pattern que les
  // refs onMoveRef/onShapesChangeRef au-dessus.
  const fenRef = useRef(fen);
  fenRef.current = fen;
  // Coup de pion vers la derniere rangee en attente du choix de la piece de
  // promotion (voir le petit selecteur rendu plus bas) : chessground a deja
  // deplace le pion visuellement (son callback "after" se declenche apres
  // l'animation), onMove n'est donc appele qu'une fois la piece choisie --
  // ou le coup annule et le plateau resynchronise sur `fen` si l'utilisateur
  // clique ailleurs.
  const [pendingPromotion, setPendingPromotion] = useState<{ from: Key; to: Key; color: Color } | null>(null);

  useEffect(() => {
    if (!elRef.current) return;
    const config: Config = {
      fen,
      turnColor,
      lastMove: lastMove ?? undefined,
      orientation: orientation ?? "white",
      coordinates: true,
      ranksPosition: "right",
      movable: {
        free: false,
        color: turnColor,
        dests,
        events: {
          after: (orig, dest) => {
            const piece = pieceAt(fenRef.current, orig);
            if ((piece === "P" || piece === "p") && (dest[1] === "1" || dest[1] === "8")) {
              setPendingPromotion({ from: orig, to: dest, color: piece === "P" ? "white" : "black" });
            } else {
              onMoveRef.current(orig, dest, null);
            }
          },
        },
      },
      drawable: {
        enabled: true,
        shapes: toDrawShapes(shapes),
        onChange: (s) => onShapesChangeRef.current(fromDrawShapes(s)),
      },
    };
    apiRef.current = Chessground(elRef.current, config);
    return () => {
      apiRef.current?.destroy();
      apiRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    apiRef.current?.set({
      fen,
      turnColor,
      lastMove: lastMove ?? undefined,
      orientation: orientation ?? "white",
      movable: { color: turnColor, dests },
      drawable: { shapes: toDrawShapes(shapes) },
    });
    // Un changement de position externe (navigation dans la notation,
    // annulation, etc.) rend un choix de promotion en attente perime.
    setPendingPromotion(null);
  }, [fen, turnColor, dests, shapes, lastMove, orientation]);

  function cancelPromotion() {
    setPendingPromotion(null);
    apiRef.current?.set({ fen: fenRef.current });
  }

  function choosePromotion(role: string) {
    if (!pendingPromotion) return;
    const { from, to } = pendingPromotion;
    setPendingPromotion(null);
    onMoveRef.current(from, to, role);
  }

  // suit la taille du conteneur (panneau dockview redimensionnable) pour
  // garder un echiquier carre qui occupe tout l'espace disponible
  useEffect(() => {
    if (!outerRef.current) return;
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (!entry) return;
      const { width, height } = entry.contentRect;
      const next = Math.max(160, Math.floor(Math.min(width, height)));
      setSize(next);
    });
    observer.observe(outerRef.current);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    apiRef.current?.redrawAll();
    onSizeChangeRef.current?.(size);
  }, [size]);

  const square = size / 8;
  // La case de promotion est toujours sur la derniere rangee, donc toujours
  // tout en haut ou tout en bas a l'ecran selon l'orientation -- le
  // selecteur s'empile depuis cette case vers le centre du plateau (comme
  // Lichess/chess.com), jamais a moitie hors cadre.
  const promotionStyle = pendingPromotion
    ? (() => {
        const file = pendingPromotion.to.charCodeAt(0) - 97;
        const rank = Number(pendingPromotion.to[1]);
        const col = orientation === "black" ? 7 - file : file;
        const atTop = orientation === "black" ? rank === 1 : rank === 8;
        const flexDirection: "column" | "column-reverse" = atTop ? "column" : "column-reverse";
        return { left: col * square, top: atTop ? 0 : size - 4 * square, flexDirection };
      })()
    : null;

  return (
    <div className={`board-wrap board-theme-${theme}`} ref={outerRef}>
      <div className="board-surface" style={{ width: size, height: size }}>
        <div ref={elRef} className="cg-wrap" style={{ width: size, height: size }} />
        {pendingPromotion && promotionStyle && (
          <>
            <div className="promotion-backdrop" onClick={cancelPromotion} />
            <div className="promotion-picker" style={{ ...promotionStyle, width: square, height: 4 * square }}>
              {PROMOTION_ROLES.map(({ role, white, black }) => (
                <button
                  key={role}
                  className="promotion-picker-choice"
                  style={{ width: square, height: square }}
                  onClick={() => choosePromotion(role)}
                >
                  {pendingPromotion.color === "white" ? white : black}
                </button>
              ))}
            </div>
          </>
        )}
      </div>
    </div>
  );
}
