import type { MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import type { ExplorerGameSummary, ExplorerGamesSort, ExplorerGamesSortColumn } from "../types";
import { ColumnResizeHandle, useColumnWidths } from "../useColumnWidths";

const EXPLORER_GAMES_COLUMNS = [
  { key: "white", default: 100 },
  { key: "whiteElo", default: 44 },
  { key: "black", default: 100 },
  { key: "blackElo", default: 44 },
  { key: "date", default: 76 },
  { key: "result", default: 48 },
];

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
  width,
  onResizeStart,
}: {
  label: string;
  column: ExplorerGamesSortColumn;
  sort: ExplorerGamesSort | null;
  onSortChange: (column: ExplorerGamesSortColumn) => void;
  width: number;
  onResizeStart: (e: MouseEvent) => void;
}) {
  const active = sort?.column === column;
  return (
    <th className="games-table-sortable" style={{ width }} onClick={() => onSortChange(column)}>
      {label} {active ? (sort!.dir === "asc" ? "▲" : "▼") : ""}
      <ColumnResizeHandle onMouseDown={onResizeStart} />
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
  const { widths, startResize, totalWidth } = useColumnWidths("explorerGamesList", EXPLORER_GAMES_COLUMNS);

  return (
    <div className="explorer-games">
      <h3 className="explorer-games-title">{t("explorer.gamesHere")}</h3>
      {error && <p className="error">{error}</p>}
      {games.length === 0 && !loading ? (
        <p className="notation-empty">{t("gamesTable.empty")}</p>
      ) : (
        <table className="explorer-table explorer-games-table" style={{ width: totalWidth }}>
          <thead>
            <tr>
              <SortableHeader
                label={t("common.white")}
                column="white"
                sort={sort}
                onSortChange={onSortChange}
                width={widths.white}
                onResizeStart={startResize("white")}
              />
              <SortableHeader
                label={t("explorer.elo")}
                column="whiteElo"
                sort={sort}
                onSortChange={onSortChange}
                width={widths.whiteElo}
                onResizeStart={startResize("whiteElo")}
              />
              <SortableHeader
                label={t("common.black")}
                column="black"
                sort={sort}
                onSortChange={onSortChange}
                width={widths.black}
                onResizeStart={startResize("black")}
              />
              <SortableHeader
                label={t("explorer.elo")}
                column="blackElo"
                sort={sort}
                onSortChange={onSortChange}
                width={widths.blackElo}
                onResizeStart={startResize("blackElo")}
              />
              <SortableHeader
                label={t("common.date")}
                column="date"
                sort={sort}
                onSortChange={onSortChange}
                width={widths.date}
                onResizeStart={startResize("date")}
              />
              <SortableHeader
                label={t("common.result")}
                column="result"
                sort={sort}
                onSortChange={onSortChange}
                width={widths.result}
                onResizeStart={startResize("result")}
              />
            </tr>
          </thead>
          <tbody>
            {games.map((g) => (
              <tr
                key={g.id}
                className={g.id === selectedGameId ? "explorer-table-row--selected" : ""}
                onClick={() => onSelectGame(g.id)}
              >
                <td style={{ width: widths.white }} title={g.white}>
                  {g.white}
                </td>
                <td style={{ width: widths.whiteElo }} className="explorer-games-elo">
                  {g.whiteElo ?? "–"}
                </td>
                <td style={{ width: widths.black }} title={g.black}>
                  {g.black}
                </td>
                <td style={{ width: widths.blackElo }} className="explorer-games-elo">
                  {g.blackElo ?? "–"}
                </td>
                <td style={{ width: widths.date }}>{g.date}</td>
                <td style={{ width: widths.result }}>{g.result}</td>
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
