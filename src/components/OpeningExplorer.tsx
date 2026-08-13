import type { OpeningMoveStat } from "../types";

interface OpeningExplorerProps {
  stats: OpeningMoveStat[];
  error: string | null;
  onSelectMove: (san: string) => void;
}

export function OpeningExplorer({ stats, error, onSelectMove }: OpeningExplorerProps) {
  if (error) {
    return <p className="error">{error}</p>;
  }

  if (stats.length === 0) {
    return <p className="notation-empty">Aucune partie de la base ne passe par cette position</p>;
  }

  const total = stats.reduce((sum, s) => sum + s.games, 0);

  return (
    <table className="explorer-table">
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
        {stats.map((s) => (
          <tr key={s.san} onClick={() => onSelectMove(s.san)}>
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
