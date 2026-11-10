import { useEffect, useMemo, useState } from "react";
import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { EcoInfo, OpeningMoveStat } from "../types";
import { compareText, useClientSort, type Comparators } from "../useClientSort";
import { ColumnResizeHandle, useColumnWidths, useFillWidth } from "../useColumnWidths";

type ExplorerStatsColumn = "move" | "games" | "whiteDrawBlack";

// Score des Blancs (gains + moitie des nulles) sur ce coup, de 0 a 1.
function whiteScore(s: OpeningMoveStat): number {
  return s.games > 0 ? (s.whiteWins + s.draws / 2) / s.games : 0;
}

// Ordre par defaut (sans tri choisi) : celui du Rust, les plus joues d'abord.
const EXPLORER_STATS_COMPARATORS: Comparators<OpeningMoveStat, ExplorerStatsColumn> = {
  move: (a, b) => compareText(a.san, b.san),
  games: (a, b) => a.games - b.games,
  whiteDrawBlack: (a, b) => whiteScore(a) - whiteScore(b) || a.games - b.games,
};

const EXPLORER_STATS_COLUMNS = [
  { key: "move", default: 70 },
  { key: "games", default: 110 },
  { key: "whiteDrawBlack", default: 160 },
];

interface OpeningExplorerProps {
  stats: OpeningMoveStat[];
  loading: boolean;
  error: string | null;
  ecoInfo: EcoInfo | null;
  onSelectMove: (san: string) => void;
}

// Nombre de coups en tete mis en avant (fond accentue) : les plus joues se
// detachent souvent nettement du reste (voir la capture d'ecran de
// reference), pas seulement le tout premier.
const TOP_MOVES_HIGHLIGHTED = 2;

function pct(part: number, total: number): number {
  return total > 0 ? Math.round((part / total) * 100) : 0;
}

export function OpeningExplorer({ stats, loading, error, ecoInfo, onSelectMove }: OpeningExplorerProps) {
  const { t } = useTranslation();
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  // Tri garde ici (pas dans ExplorerTable, demontee quand la position n'a
  // aucun coup) : il reste le meme d'une position a l'autre.
  const { sorted, sort, toggleSort, sortArrow } = useClientSort(stats, EXPLORER_STATS_COMPARATORS);

  useEffect(() => {
    setHighlightedIndex(0);
  }, [stats, sort]);

  return (
    <div className="explorer-table-wrap">
      {ecoInfo && (
        <div className="explorer-eco-header">
          <span className="explorer-eco-code">{ecoInfo.eco}</span>
          <span className="explorer-eco-name">{ecoInfo.name}</span>
        </div>
      )}
      {error ? (
        <p className="error">{error}</p>
      ) : stats.length === 0 && loading ? (
        // distingue "encore en train de charger" de "vraiment aucune partie"
        // (explorer.empty) -- sans ca, le tout premier chargement (ex. juste
        // apres "Se preparer contre") affichait "aucune partie" le temps que
        // la reponse revienne, trompeur (rapporte par l'utilisateur).
        <p className="status-message">{t("common.loading")}</p>
      ) : stats.length === 0 ? (
        <p className="notation-empty">{t("explorer.empty")}</p>
      ) : (
        <ExplorerTable
          stats={sorted}
          highlightedIndex={highlightedIndex}
          setHighlightedIndex={setHighlightedIndex}
          onSelectMove={onSelectMove}
          onSortChange={toggleSort}
          sortArrow={sortArrow}
        />
      )}
    </div>
  );
}

function ExplorerTable({
  stats,
  highlightedIndex,
  setHighlightedIndex,
  onSelectMove,
  onSortChange,
  sortArrow,
}: {
  stats: OpeningMoveStat[];
  highlightedIndex: number;
  setHighlightedIndex: (i: number) => void;
  onSelectMove: (san: string) => void;
  onSortChange: (column: ExplorerStatsColumn) => void;
  sortArrow: (column: ExplorerStatsColumn) => string;
}) {
  const { t } = useTranslation();
  const { widths: baseWidths, startResize } = useColumnWidths("explorerStats", EXPLORER_STATS_COLUMNS);
  // la place libre va a la barre Blancs/nulles/Noirs, plus lisible large
  const { tableRef, fitted: widths, tableWidth } = useFillWidth(baseWidths, ["whiteDrawBlack"]);
  // Les coups les plus joues restent mis en avant quel que soit le tri
  // affiche (pas simplement les deux premieres lignes).
  const topMoves = useMemo(
    () => new Set([...stats].sort((a, b) => b.games - a.games).slice(0, TOP_MOVES_HIGHLIGHTED).map((s) => s.san)),
    [stats]
  );
  const total = stats.reduce((sum, s) => sum + s.games, 0);
  const totalWhite = stats.reduce((sum, s) => sum + s.whiteWins, 0);
  const totalDraws = stats.reduce((sum, s) => sum + s.draws, 0);
  const totalBlack = stats.reduce((sum, s) => sum + s.blackWins, 0);

  function handleKeyDown(e: KeyboardEvent<HTMLTableElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      e.stopPropagation();
      setHighlightedIndex((highlightedIndex + 1) % stats.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      e.stopPropagation();
      setHighlightedIndex((highlightedIndex - 1 + stats.length) % stats.length);
    } else if (e.key === "Enter" || e.key === "ArrowRight") {
      e.preventDefault();
      e.stopPropagation();
      onSelectMove(stats[highlightedIndex].san);
    }
  }

  return (
    // .explorer-table est partagee avec la liste de parties et la table FIDE
    // (voir App.css) : .explorer-stats-table ne scope que les styles propres
    // aux stats d'ouverture (barre, coups en tete, ligne total) pour ne
    // jamais affecter ces autres tables.
    <table
      ref={tableRef}
      className="explorer-table explorer-stats-table resizable-table"
      style={{ width: tableWidth }}
      tabIndex={0}
      onKeyDown={handleKeyDown}
    >
      <thead>
        <tr>
          <th className="games-table-sortable" style={{ width: widths.move }} onClick={() => onSortChange("move")}>
            {t("explorer.move")} {sortArrow("move")}
            <ColumnResizeHandle onMouseDown={startResize("move")} />
          </th>
          <th className="games-table-sortable" style={{ width: widths.games }} onClick={() => onSortChange("games")}>
            {t("explorer.games")} {sortArrow("games")}
            <ColumnResizeHandle onMouseDown={startResize("games")} />
          </th>
          <th
            className="games-table-sortable"
            style={{ width: widths.whiteDrawBlack }}
            title={t("explorer.whiteScoreSortHint")}
            onClick={() => onSortChange("whiteDrawBlack")}
          >
            {t("explorer.whiteDrawBlack")} {sortArrow("whiteDrawBlack")}
            <ColumnResizeHandle onMouseDown={startResize("whiteDrawBlack")} />
          </th>
        </tr>
      </thead>
      <tbody>
        {stats.map((s, index) => {
          const rowClasses = [
            index === highlightedIndex ? "explorer-table-row--selected" : "",
            topMoves.has(s.san) ? "explorer-table-row--top" : "",
          ]
            .filter(Boolean)
            .join(" ");
          return (
            <tr
              key={s.san}
              className={rowClasses}
              onClick={() => {
                setHighlightedIndex(index);
                onSelectMove(s.san);
              }}
            >
              <td className="explorer-move">{s.san}</td>
              <td className="explorer-games-cell">
                <span className="explorer-games-pct">{pct(s.games, total)}%</span>
                <span className="explorer-games-count">{s.games.toLocaleString()}</span>
              </td>
              <td>
                <ExplorerBar white={s.whiteWins} draws={s.draws} black={s.blackWins} />
              </td>
            </tr>
          );
        })}
      </tbody>
      <tfoot>
        <tr className="explorer-table-row--total">
          <td>Σ</td>
          <td className="explorer-games-cell">
            <span className="explorer-games-pct">100%</span>
            <span className="explorer-games-count">{total.toLocaleString()}</span>
          </td>
          <td>
            <ExplorerBar white={totalWhite} draws={totalDraws} black={totalBlack} />
          </td>
        </tr>
      </tfoot>
    </table>
  );
}

function ExplorerBar({ white, draws, black }: { white: number; draws: number; black: number }) {
  const total = white + draws + black;
  const whitePct = pct(white, total);
  const drawPct = pct(draws, total);
  // le dernier segment absorbe l'arrondi (evite 99%/101% sur l'ensemble des
  // trois barres) plutot que d'etre recalcule independamment.
  const blackPct = Math.max(0, 100 - whitePct - drawPct);
  return (
    <div className="explorer-bar">
      <span className="explorer-bar-white" style={{ width: `${whitePct}%` }}>
        {whitePct > 0 && `${whitePct}%`}
      </span>
      <span className="explorer-bar-draw" style={{ width: `${drawPct}%` }} />
      <span className="explorer-bar-black" style={{ width: `${blackPct}%` }}>
        {blackPct > 0 && `${blackPct}%`}
      </span>
    </div>
  );
}
