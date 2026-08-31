import { useState } from "react";
import { useTranslation } from "react-i18next";
import { GAME_DRAG_MIME, type GameDragPayload } from "../dnd";
import type { BaseInfo } from "../types";

interface BasesTableProps {
  bases: BaseInfo[];
  selectedPath?: string | null;
  onSelect: (base: BaseInfo) => void;
  // ctrlKey: si vraie, l'appelant doit copier directement sans demander --
  // convention glisser-deposer (Ctrl = copier). Sinon, a l'appelant de
  // proposer un choix explicite deplacer/copier.
  onDropGames?: (base: BaseInfo, ids: number[], fromPath: string, ctrlKey: boolean) => void;
}

export function BasesTable({ bases, selectedPath, onSelect, onDropGames }: BasesTableProps) {
  const { t } = useTranslation();
  const [dragOverPath, setDragOverPath] = useState<string | null>(null);

  if (bases.length === 0) {
    return <p className="notation-empty">{t("basesTable.empty")}</p>;
  }

  return (
    <table className="explorer-table">
      <thead>
        <tr>
          <th>{t("basesTable.name")}</th>
          <th>{t("basesTable.games")}</th>
          <th>{t("basesTable.location")}</th>
        </tr>
      </thead>
      <tbody>
        {bases.map((b) => (
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
  );
}
