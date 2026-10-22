import { useEffect, useState } from "react";
import type { KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";
import type { EcoInfo, OpeningMoveStat } from "../types";
import { ColumnResizeHandle, useColumnWidths } from "../useColumnWidths";

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

  useEffect(() => {
    setHighlightedIndex(0);
  }, [stats]);

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
        <ExplorerTable stats={stats} highlightedIndex={highlightedIndex} setHighlightedIndex={setHighlightedIndex} onSelectMove={onSelectMove} />
      )}
    </div>
  );
}

function ExplorerTable({
  stats,
  highlightedIndex,
  setHighlightedIndex,
  onSelectMove,
}: {
  stats: OpeningMoveStat[];
  highlightedIndex: number;
  setHighlightedIndex: (i: number) => void;
  onSelectMove: (san: string) => void;
}) {
  const { t } = useTranslation();
  const { widths, startResize, totalWidth } = useColumnWidths("explorerStats", EXPLORER_STATS_COLUMNS);
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
      className="explorer-table explorer-stats-table resizable-table"
      style={{ width: totalWidth }}
      tabIndex={0}
      onKeyDown={handleKeyDown}
    >
      <thead>
        <tr>
          <th style={{ width: widths.move }}>
            {t("explorer.move")}
            <ColumnResizeHandle onMouseDown={startResize("move")} />
          </th>
          <th style={{ width: widths.games }}>
            {t("explorer.games")}
            <ColumnResizeHandle onMouseDown={startResize("games")} />
          </th>
          <th style={{ width: widths.whiteDrawBlack }}>
            {t("explorer.whiteDrawBlack")}
            <ColumnResizeHandle onMouseDown={startResize("whiteDrawBlack")} />
          </th>
        </tr>
      </thead>
      <tbody>
        {stats.map((s, index) => {
          const rowClasses = [
            index === highlightedIndex ? "explorer-table-row--selected" : "",
            index < TOP_MOVES_HIGHLIGHTED ? "explorer-table-row--top" : "",
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
