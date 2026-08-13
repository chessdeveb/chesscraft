import { useEffect, useRef } from "react";
import { Chessground } from "@lichess-org/chessground";
import type { Api } from "@lichess-org/chessground/api";
import type { Config } from "@lichess-org/chessground/config";
import type { Color, Dests, Key } from "@lichess-org/chessground/types";

interface BoardProps {
  fen: string;
  turnColor: Color;
  dests: Dests;
  onMove: (from: Key, to: Key) => void;
}

export function Board({ fen, turnColor, dests, onMove }: BoardProps) {
  const elRef = useRef<HTMLDivElement>(null);
  const apiRef = useRef<Api | null>(null);
  const onMoveRef = useRef(onMove);
  onMoveRef.current = onMove;

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
    });
  }, [fen, turnColor, dests]);

  return (
    <div className="board-wrap">
      <div ref={elRef} className="cg-wrap" />
    </div>
  );
}
