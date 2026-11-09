import { useState } from "react";
import { useTranslation } from "react-i18next";
import { GAME_DRAG_MIME, type GameDragPayload } from "../dnd";
import type { BaseInfo } from "../types";
import { compareText, useClientSort, type Comparators } from "../useClientSort";
import { ColumnResizeHandle, useColumnWidths } from "../useColumnWidths";

const BASES_TABLE_COLUMNS = [
  { key: "name", default: 160 },
  { key: "games", default: 70 },
  { key: "location", default: 320 },
];

// Liste courte et deja entierement chargee (list_bases) : triee cote
// interface. Sans tri choisi, l'ordre d'enregistrement est garde.
const BASES_COMPARATORS: Comparators<BaseInfo, "name" | "games" | "location"> = {
  name: (a, b) => compareText(a.name, b.name),
  games: (a, b) => a.games - b.games || compareText(a.name, b.name),
  location: (a, b) => compareText(a.path, b.path),
};

interface BasesTableProps {
  bases: BaseInfo[];
  // true tant que le tout premier chargement (list_bases) n'est pas revenu
  // -- sans ca, la liste vide par defaut (avant que l'invoke ne reponde)
  // affichait "Aucune base pour l'instant" pendant ce court instant, comme
  // si l'utilisateur n'avait vraiment aucune base enregistree (rapporte par
  // l'utilisateur comme trompeur, cf. OpeningExplorer/explorerStatsLoading
  // pour le meme correctif ailleurs). Optionnel : les appelants qui ne
  // chargent jamais rien (deja tout en props) n'ont pas besoin de le passer.
  loading?: boolean;
  selectedPath?: string | null;
  onSelect: (base: BaseInfo) => void;
  // ctrlKey: si vraie, l'appelant doit copier directement sans demander --
  // convention glisser-deposer (Ctrl = copier). Sinon, a l'appelant de
  // proposer un choix explicite deplacer/copier.
  onDropGames?: (base: BaseInfo, ids: number[], fromPath: string, ctrlKey: boolean) => void;
}

export function BasesTable({ bases, loading, selectedPath, onSelect, onDropGames }: BasesTableProps) {
  const { t } = useTranslation();
  const [dragOverPath, setDragOverPath] = useState<string | null>(null);
  const { widths, startResize, totalWidth } = useColumnWidths("basesTable", BASES_TABLE_COLUMNS);
  const { sorted: sortedBases, toggleSort: onSortChange, sortArrow } = useClientSort(bases, BASES_COMPARATORS);

  if (bases.length === 0) {
    return loading ? <p className="status-message">{t("common.loading")}</p> : <p className="notation-empty">{t("basesTable.empty")}</p>;
  }

  return (
    <div className="resizable-table-wrap">
    <table className="explorer-table resizable-table" style={{ width: totalWidth }}>
      <thead>
        <tr>
          <th className="games-table-sortable" style={{ width: widths.name }} onClick={() => onSortChange("name")}>
            {t("basesTable.name")} {sortArrow("name")}
            <ColumnResizeHandle onMouseDown={startResize("name")} />
          </th>
          <th className="games-table-sortable" style={{ width: widths.games }} onClick={() => onSortChange("games")}>
            {t("basesTable.games")} {sortArrow("games")}
            <ColumnResizeHandle onMouseDown={startResize("games")} />
          </th>
          <th className="games-table-sortable" style={{ width: widths.location }} onClick={() => onSortChange("location")}>
            {t("basesTable.location")} {sortArrow("location")}
            <ColumnResizeHandle onMouseDown={startResize("location")} />
          </th>
        </tr>
      </thead>
      <tbody>
        {sortedBases.map((b) => (
          <tr
            key={b.path}
            className={
              (selectedPath === b.path ? "explorer-table-row--selected" : "") +
              (dragOverPath === b.path ? " explorer-table-row--drop-target" : "")
            }
            onClick={() => onSelect(b)}
            onDragOver={(e) => {
              if (!onDropGames || !e.dataTransfer.types.includes(GAME_DRAG_MIME)) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = e.ctrlKey ? "copy" : "move";
            }}
            onDragEnter={(e) => {
              if (!onDropGames || !e.dataTransfer.types.includes(GAME_DRAG_MIME)) return;
              setDragOverPath(b.path);
            }}
            onDragLeave={() => setDragOverPath((prev) => (prev === b.path ? null : prev))}
            onDrop={(e) => {
              setDragOverPath(null);
              if (!onDropGames) return;
              const raw = e.dataTransfer.getData(GAME_DRAG_MIME);
              if (!raw) return;
              e.preventDefault();
              const payload = JSON.parse(raw) as GameDragPayload;
              if (payload.fromPath === b.path) return;
              onDropGames(b, payload.ids, payload.fromPath, e.ctrlKey);
            }}
          >
            <td>
              {b.isReference && <span title={t("home.referenceBase")}>📚 </span>}
              {b.missing && <span title={t("home.fileMissing")}>⚠️ </span>}
              {b.name}
            </td>
            <td>{b.games}</td>
            <td className="bases-table-path">{b.path}</td>
          </tr>
        ))}
      </tbody>
    </table>
    </div>
  );
}
