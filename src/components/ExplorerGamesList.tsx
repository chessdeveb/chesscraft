import { useTranslation } from "react-i18next";
import type { GameSummary } from "../types";

interface ExplorerGamesListProps {
  games: GameSummary[];
  total: number;
  page: number;
  pageSize: number;
  loading: boolean;
  error: string | null;
  onPageChange: (page: number) => void;
  selectedGameId: number | null;
  onSelectGame: (id: number) => void;
}

export function ExplorerGamesList({
  games,
  total,
  page,
  pageSize,
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
              <th>{t("common.white")}</th>
              <th>{t("common.black")}</th>
              <th>{t("common.date")}</th>
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
                <td>{g.white}</td>
                <td>{g.black}</td>
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
