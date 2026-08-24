import type { GamesSort, GamesSortColumn } from "../HomeContext";

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
  sort?: GamesSort | null;
  onSortChange?: (column: GamesSortColumn) => void;
}

function SortableHeader({
  label,
  column,
  sort,
  onSortChange,
}: {
  label: string;
  column: GamesSortColumn;
  sort?: GamesSort | null;
  onSortChange?: (column: GamesSortColumn) => void;
}) {
  if (!onSortChange) return <th>{label}</th>;
  const active = sort?.column === column;
  return (
    <th className="games-table-sortable" onClick={() => onSortChange(column)}>
      {label} {active ? (sort!.dir === "asc" ? "▲" : "▼") : ""}
    </th>
  );
}

export function GamesTable({ games, onOpen, onDelete, sort, onSortChange }: GamesTableProps) {
  if (games.length === 0) {
    return <p className="notation-empty">Aucune partie ici</p>;
  }

  return (
    <table className="explorer-table">
      <thead>
        <tr>
          <SortableHeader label="Blancs" column="white" sort={sort} onSortChange={onSortChange} />
          <SortableHeader label="Noirs" column="black" sort={sort} onSortChange={onSortChange} />
          <SortableHeader label="Date" column="date" sort={sort} onSortChange={onSortChange} />
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
