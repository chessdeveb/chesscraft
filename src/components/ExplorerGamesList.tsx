import { useTranslation } from "react-i18next";
import type { ExplorerGameSummary, ExplorerGamesSort, ExplorerGamesSortColumn } from "../types";

interface ExplorerGamesListProps {
  games: ExplorerGameSummary[];
  total: number;
  page: number;
  pageSize: number;
  sort: ExplorerGamesSort | null;
  onSortChange: (column: ExplorerGamesSortColumn) => void;
  loading: boolean;
  error: string | null;
  onPageChange: (page: number) => void;
  selectedGameId: number | null;
  onSelectGame: (id: number) => void;
}

// Meme principe que SortableHeader (GamesTable.tsx) : clic pour trier par
// cette colonne, un second clic inverse le sens.
function SortableHeader({
  label,
  column,
  sort,
  onSortChange,
}: {
  label: string;
  column: ExplorerGamesSortColumn;
  sort: ExplorerGamesSort | null;
  onSortChange: (column: ExplorerGamesSortColumn) => void;
}) {
  const active = sort?.column === column;
  return (
    <th className="games-table-sortable" onClick={() => onSortChange(column)}>
      {label} {active ? (sort!.dir === "asc" ? "▲" : "▼") : ""}
    </th>
  );
}

export function ExplorerGamesList({
  games,
  total,
  page,
  pageSize,
  sort,
  onSortChange,
  loading,
  error,
  onPageChange,
  selectedGameId,
  onSelectGame,
}: ExplorerGamesListProps) {
  const { t } = useTranslation();

  return (
    <div className="explorer-games">
      <h3 className="explorer-games-title">{t("explorer.gamesHere")}</h3>
      {error && <p className="error">{error}</p>}
      {games.length === 0 && !loading ? (
        <p className="notation-empty">{t("gamesTable.empty")}</p>
      ) : (
        <table className="explorer-table explorer-games-table">
          <thead>
            <tr>
              <SortableHeader label={t("common.white")} column="white" sort={sort} onSortChange={onSortChange} />
              <SortableHeader label={t("explorer.elo")} column="whiteElo" sort={sort} onSortChange={onSortChange} />
              <SortableHeader label={t("common.black")} column="black" sort={sort} onSortChange={onSortChange} />
              <SortableHeader label={t("explorer.elo")} column="blackElo" sort={sort} onSortChange={onSortChange} />
              <SortableHeader label={t("common.date")} column="date" sort={sort} onSortChange={onSortChange} />
              <th>{t("common.result")}</th>
            </tr>
          </thead>
          <tbody>
            {games.map((g) => (
              <tr
                key={g.id}
                className={g.id === selectedGameId ? "explorer-table-row--selected" : ""}
                onClick={() => onSelectGame(g.id)}
              >
                <td title={g.white}>{g.white}</td>
                <td className="explorer-games-elo">{g.whiteElo ?? "–"}</td>
                <td title={g.black}>{g.black}</td>
                <td className="explorer-games-elo">{g.blackElo ?? "–"}</td>
                <td>{g.date}</td>
                <td>{g.result}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {total > pageSize && (
        <div className="games-pagination">
          <button disabled={loading || page === 0} onClick={() => onPageChange(page - 1)}>
            {t("home.previous")}
          </button>
          <span>
            {t("home.pageOf", {
              from: page * pageSize + 1,
              to: Math.min((page + 1) * pageSize, total),
              total,
            })}
          </span>
          <button disabled={loading || (page + 1) * pageSize >= total} onClick={() => onPageChange(page + 1)}>
            {t("home.next")}
          </button>
        </div>
      )}
    </div>
  );
}
