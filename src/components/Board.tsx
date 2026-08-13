import { useEffect, useRef } from "react";
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

export function Board({ fen, turnColor, dests, shapes, onMove, onShapesChange }: BoardProps) {
  const elRef = useRef<HTMLDivElement>(null);
  const apiRef = useRef<Api | null>(null);
  const onMoveRef = useRef(onMove);
  onMoveRef.current = onMove;
  const onShapesChangeRef = useRef(onShapesChange);
  onShapesChangeRef.current = onShapesChange;

  useEffect(() => {
    if (!elRef.current) return;
    const config: Config = {
      fen,
      turnColor,
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
      movable: { color: turnColor, dests },
      drawable: { shapes: toDrawShapes(shapes) },
    });
  }, [fen, turnColor, dests, shapes]);

  return (
    <div className="board-wrap">
      <div ref={elRef} className="cg-wrap" />
    </div>
  );
}
