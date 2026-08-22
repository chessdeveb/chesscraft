import { useEffect, useState } from "react";
import type { KeyboardEvent } from "react";
import type { OpeningMoveStat } from "../types";

interface OpeningExplorerProps {
  stats: OpeningMoveStat[];
  error: string | null;
  onSelectMove: (san: string) => void;
}

export function OpeningExplorer({ stats, error, onSelectMove }: OpeningExplorerProps) {
  const [highlightedIndex, setHighlightedIndex] = useState(0);

  useEffect(() => {
    setHighlightedIndex(0);
  }, [stats]);

  if (error) {
    return <p className="error">{error}</p>;
  }

  if (stats.length === 0) {
    return <p className="notation-empty">Aucune partie de la base ne passe par cette position</p>;
  }

  const total = stats.reduce((sum, s) => sum + s.games, 0);

  function handleKeyDown(e: KeyboardEvent<HTMLTableElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      setHighlightedIndex((i) => (i + 1) % stats.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      setHighlightedIndex((i) => (i - 1 + stats.length) % stats.length);
    } else if (e.key === "Enter" || e.key === "ArrowRight") {
      e.preventDefault();
      e.stopPropagation();
      onSelectMove(stats[highlightedIndex].san);
    }
  }

  return (
    <table className="explorer-table" tabIndex={0} onKeyDown={handleKeyDown}>
      <thead>
        <tr>
          <th>Coup</th>
          <th>Parties</th>
          <th>%</th>
          <th>Blancs</th>
          <th>Nulles</th>
          <th>Noirs</th>
        </tr>
      </thead>
      <tbody>
        {stats.map((s, index) => (
          <tr
            key={s.san}
            className={index === highlightedIndex ? "explorer-table-row--selected" : ""}
            onClick={() => {
              setHighlightedIndex(index);
              onSelectMove(s.san);
            }}
          >
            <td className="explorer-move">{s.san}</td>
            <td>{s.games}</td>
            <td>{total > 0 ? Math.round((s.games / total) * 100) : 0}%</td>
            <td>{s.whiteWins}</td>
            <td>{s.draws}</td>
            <td>{s.blackWins}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
