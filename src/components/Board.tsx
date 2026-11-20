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
  /** Fleche du geste "clic gauche maintenu + clic droit" : meilleure reponse du
      moteur au coup en train d'etre joue (`move` : piece glissee et case
      survolee) -- ou, sans coup en cours, meilleur coup de la position
      affichee (`move` null : c'est alors la reponse a un coup qui vient d'etre
      joue). `fen` = position pour laquelle la fleche est le meilleur coup
      (celle d'apres le coup envisage), null = la position affichee. Resultat
      null : pas de fleche. Temporaire, jamais ajoutee aux annotations. */
  requestHint?: (fen: string, move: { from: Key; to: Key; promotion: string | null } | null) => Promise<{ fen: string | null; from: Key; to: Key } | null>;
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

export function Board({ fen, turnColor, dests, shapes, theme, lastMove, orientation, onMove, onShapesChange, onSizeChange, requestHint }: BoardProps) {
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

  // Derniers props, pour resynchroniser chessground hors du cycle de rendu
  // (annulation d'une promotion, voir cancelPromotion).
  const syncRef = useRef({ turnColor, dests, shapes, lastMove, orientation });
  syncRef.current = { turnColor, dests, shapes, lastMove, orientation };

  function syncBoard() {
    const s = syncRef.current;
    apiRef.current?.set({
      fen: fenRef.current,
      turnColor: s.turnColor,
      lastMove: s.lastMove ?? undefined,
      orientation: s.orientation ?? "white",
      movable: { color: s.turnColor, dests: s.dests },
      drawable: { shapes: toDrawShapes(s.shapes) },
    });
  }

  // Geste "clic gauche maintenu + clic droit" : fleche de la meilleure
  // reponse du moteur. Pendant que le bouton gauche est enfonce sur
  // l'echiquier, un appui sur le bouton droit affiche la fleche : si une
  // piece est en train d'etre glissee, c'est la reponse au coup qu'elle
  // s'apprete a jouer (case survolee) ; sinon le meilleur coup de la position
  // affichee. Le mousedown droit est intercepte en phase de capture pour que
  // chessground n'y voie pas le debut d'une fleche dessinee a la main, qui
  // serait ajoutee aux annotations. La fleche s'efface au prochain clic
  // gauche sur l'echiquier ou quand la position change (sauf si c'est
  // justement la position d'apres le coup envisage, voir l'effet sur fen).
  const requestHintRef = useRef(requestHint);
  requestHintRef.current = requestHint;
  const destsRef = useRef(dests);
  destsRef.current = dests;
  // position pour laquelle la fleche affichee est le meilleur coup
  const hintFenRef = useRef<string | null>(null);
  const hintTokenRef = useRef(0);
  useEffect(() => {
    let leftDownOnBoard = false;
    let swallowContextMenu = false;
    // case ou la piece glissee a ete lachee (undefined = hors echiquier),
    // null tant que le bouton gauche est enfonce
    let releasedOn: Key | undefined | null = null;
    // case d'arrivee du coup dont la fleche montre la reponse (glissement)
    let hintDest: Key | null = null;
    function clearHint() {
      hintTokenRef.current += 1;
      hintFenRef.current = null;
      hintDest = null;
      apiRef.current?.setAutoShapes([]);
    }
    function onMouseDown(e: MouseEvent) {
      const inBoard = outerRef.current?.contains(e.target as Node) ?? false;
      if (e.button === 0) {
        leftDownOnBoard = inBoard;
        releasedOn = null;
        if (inBoard) clearHint();
      } else if (e.button === 2 && leftDownOnBoard && (e.buttons & 1) === 1) {
        e.preventDefault();
        e.stopPropagation();
        swallowContextMenu = true;
        const request = requestHintRef.current;
        const api = apiRef.current;
        if (!request || !api) return;
        clearHint();
        const token = hintTokenRef.current;
        const shownFen = fenRef.current;
        // coup en cours de glissement : piece saisie + case sous le curseur
        let move: { from: Key; to: Key; promotion: string | null } | null = null;
        const dragged = api.state.draggable.current;
        if (dragged) {
          const from = dragged.orig;
          const to = api.getKeyAtDomPos([e.clientX, e.clientY]);
          const legal = to !== undefined && (destsRef.current.get(from) ?? []).includes(to);
          if (!legal) return; // curseur hors des cases permises : pas de fleche
          const piece = pieceAt(shownFen, from);
          const promotes = (piece === "P" || piece === "p") && (to[1] === "1" || to[1] === "8");
          move = { from, to, promotion: promotes ? "q" : null };
          hintDest = to;
        }
        request(shownFen, move)
          .then((hint) => {
            // abandonne si un autre clic a eu lieu entre-temps ou si la
            // position a change sans etre celle attendue
            if (!hint || token !== hintTokenRef.current) return;
            // piece deja lachee ailleurs que sur la case du coup envisage
            // (remise a sa place, coup annule) : la fleche n'a plus de sens
            if (releasedOn !== null && move && releasedOn !== move.to) return;
            const current = fenRef.current.split(" ").slice(0, 2).join(" ");
            const expected = (hint.fen ?? shownFen).split(" ").slice(0, 2).join(" ");
            const before = shownFen.split(" ").slice(0, 2).join(" ");
            if (current !== expected && current !== before) return;
            hintFenRef.current = hint.fen ?? shownFen;
            apiRef.current?.setAutoShapes([{ orig: hint.from, dest: hint.to, brush: "blue" }]);
          })
          .catch(() => {});
      }
    }
    function onMouseUp(e: MouseEvent) {
      if (e.button === 0) {
        leftDownOnBoard = false;
        // Piece remise sur sa case, lachee hors de l'echiquier ou sur une
        // autre case que celle du coup envisage : le coup n'est pas joue (ou
        // c'en est un autre), la fleche de la reponse est perimee. Si le coup
        // est joue, la fleche reste (voir l'effet sur fen).
        const api = apiRef.current;
        releasedOn = api ? api.getKeyAtDomPos([e.clientX, e.clientY]) : undefined;
        if (hintDest !== null && releasedOn !== hintDest) clearHint();
      } else if (leftDownOnBoard && (e.buttons & 1) === 1) {
        // Relacher le clic droit pendant que le gauche est encore enfonce :
        // chessground prendrait ce mouseup pour la fin du glissement et
        // jouerait le coup. On l'avale : la piece reste saisie, et lacher le
        // gauche sur sa case de depart (ou hors de l'echiquier) annule le coup.
        e.preventDefault();
        e.stopPropagation();
      }
    }
    function onContextMenu(e: MouseEvent) {
      if (swallowContextMenu) {
        e.preventDefault();
        swallowContextMenu = false;
      }
    }
    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("mouseup", onMouseUp, true);
    window.addEventListener("contextmenu", onContextMenu, true);
    return () => {
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("mouseup", onMouseUp, true);
      window.removeEventListener("contextmenu", onContextMenu, true);
    };
  }, []);

  // lastMove est un tableau recree a chaque rendu du parent : on dépend de ses
  // cases, sinon chaque mise a jour du moteur (plusieurs par seconde)
  // relancerait cet effet, fermerait le choix de promotion en cours et
  // effacerait la fleche du meilleur coup.
  const lastFrom = lastMove?.[0];
  const lastTo = lastMove?.[1];
  useEffect(() => {
    syncBoard();
    // Un changement de position externe (navigation dans la notation,
    // annulation, etc.) rend un choix de promotion en attente perime.
    setPendingPromotion(null);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fen, turnColor, dests, shapes, lastFrom, lastTo, orientation]);

  // Une fleche reste affichee quand la nouvelle position est celle pour
  // laquelle elle a ete calculee (le coup envisage vient d'etre lache : la
  // fleche est alors la reponse a ce coup) ; sinon elle est perimee.
  useEffect(() => {
    const keep = hintFenRef.current;
    const same = keep !== null && keep.split(" ").slice(0, 2).join(" ") === fen.split(" ").slice(0, 2).join(" ");
    if (!same) {
      hintTokenRef.current += 1;
      hintFenRef.current = null;
      apiRef.current?.setAutoShapes([]);
    }
  }, [fen]);

  // Chessground a deja joue le coup de son cote : il a rendu le trait a
  // l'adversaire et vide les coups permis. Remettre seulement le FEN
  // laissait donc l'echiquier bloque -- le pion ne pouvait plus etre repousse
  // et le selecteur ne reapparaissait pas. On restaure tout l'etat.
  function cancelPromotion() {
    setPendingPromotion(null);
    syncBoard();
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
                  style={{ width: square, height: square, fontSize: square * 0.85 }}
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
