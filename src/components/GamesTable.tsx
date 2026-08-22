export interface GameRow {
  key: number;
  white: string;
  black: string;
  date: string;
  result: string;
  event: string;
}

interface GamesTableProps {
  games: GameRow[];
  onOpen: (key: number) => void;
  onDelete?: (key: number) => void;
}

export function GamesTable({ games, onOpen, onDelete }: GamesTableProps) {
  if (games.length === 0) {
    return <p className="notation-empty">Aucune partie ici</p>;
  }

  return (
    <table className="explorer-table">
      <thead>
        <tr>
          <th>Blancs</th>
          <th>Noirs</th>
          <th>Date</th>
          <th>Résultat</th>
          <th>Évènement</th>
          {onDelete && <th></th>}
        </tr>
      </thead>
      <tbody>
        {games.map((g) => (
          <tr key={g.key} onDoubleClick={() => onOpen(g.key)}>
            <td>{g.white}</td>
            <td>{g.black}</td>
            <td>{g.date}</td>
            <td>{g.result}</td>
            <td>{g.event}</td>
            {onDelete && (
              <td>
                <button
                  className="games-table-delete"
                  title="Supprimer cette partie"
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete(g.key);
                  }}
                >
                  🗑
                </button>
              </td>
            )}
          </tr>
        ))}
      </tbody>
    </table>
  );
}
