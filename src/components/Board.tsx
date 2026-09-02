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
  onMove: (from: Key, to: Key) => void;
  onShapesChange: (shapes: ShapeDto[]) => void;
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

export function Board({ fen, turnColor, dests, shapes, theme, lastMove, onMove, onShapesChange }: BoardProps) {
  const outerRef = useRef<HTMLDivElement>(null);
  const elRef = useRef<HTMLDivElement>(null);
  const apiRef = useRef<Api | null>(null);
  const onMoveRef = useRef(onMove);
  onMoveRef.current = onMove;
  const onShapesChangeRef = useRef(onShapesChange);
  onShapesChangeRef.current = onShapesChange;
  const [size, setSize] = useState(480);

  useEffect(() => {
    if (!elRef.current) return;
    const config: Config = {
      fen,
      turnColor,
      lastMove: lastMove ?? undefined,
      coordinates: true,
      ranksPosition: "right",
      movable: {
        free: false,
        color: turnColor,
        dests,
        events: {
          after: (orig, dest) => onMoveRef.current(orig, dest),
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
      movable: { color: turnColor, dests },
      drawable: { shapes: toDrawShapes(shapes) },
    });
  }, [fen, turnColor, dests, shapes, lastMove]);

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
  }, [size]);

  return (
    <div className={`board-wrap board-theme-${theme}`} ref={outerRef}>
      <div ref={elRef} className="cg-wrap" style={{ width: size, height: size }} />
    </div>
  );
}
