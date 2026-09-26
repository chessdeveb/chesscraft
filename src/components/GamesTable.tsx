import { Fragment, useRef, useState, type DragEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { ContextMenu, type ContextMenuItem } from "./ContextMenu";
import { GAME_DRAG_MIME } from "../dnd";
import type { GamesSort, GamesSortColumn } from "../HomeContext";

export interface GameRow {
  key: number;
  white: string;
  black: string;
  date: string;
  result: string;
  event: string;
  whiteElo?: number | null;
  blackElo?: number | null;
}

interface GamesTableProps {
  games: GameRow[];
  onOpen: (key: number) => void;
  onDelete?: (key: number) => void;
  onMove?: (key: number) => void;
  sort?: GamesSort | null;
  onSortChange?: (column: GamesSortColumn) => void;
  // presence active la selection multiple, le glisser-deposer et le menu
  // contextuel -- absent pour la liste des parties d'un fichier PGN, qui ne
  // peut ni etre deplacee ni etre la cible d'un deplacement.
  sourcePath?: string;
  selectedKeys?: Set<number>;
  onSelectionChange?: (keys: Set<number>) => void;
  buildContextMenuItems?: (ids: number[]) => ContextMenuItem[];
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

export function GamesTable({
  games,
  onOpen,
  onDelete,
  onMove,
  sort,
  onSortChange,
  sourcePath,
  selectedKeys,
  onSelectionChange,
  buildContextMenuItems,
}: GamesTableProps) {
  const { t } = useTranslation();
  const lastIndexRef = useRef<number | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; ids: number[] } | null>(null);

  if (games.length === 0) {
    return <p className="notation-empty">{t("gamesTable.empty")}</p>;
  }

  function handleRowClick(e: MouseEvent, key: number, index: number) {
    if (!onSelectionChange) return;
    if (e.shiftKey && lastIndexRef.current !== null) {
      const [from, to] = [lastIndexRef.current, index].sort((a, b) => a - b);
      onSelectionChange(new Set(games.slice(from, to + 1).map((r) => r.key)));
    } else if (e.ctrlKey || e.metaKey) {
      const next = new Set(selectedKeys ?? []);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      onSelectionChange(next);
      lastIndexRef.current = index;
    } else {
      onSelectionChange(new Set([key]));
      lastIndexRef.current = index;
    }
  }

  function handleDragStart(e: DragEvent, key: number, selected: boolean) {
    if (!sourcePath) return;
    const ids = selected && selectedKeys && selectedKeys.size > 1 ? Array.from(selectedKeys) : [key];
    if (!selected) onSelectionChange?.(new Set([key]));
    e.dataTransfer.effectAllowed = "copyMove";
    e.dataTransfer.setData(GAME_DRAG_MIME, JSON.stringify({ ids, fromPath: sourcePath }));
  }

  function handleContextMenu(e: MouseEvent, key: number, selected: boolean) {
    if (!buildContextMenuItems) return;
    e.preventDefault();
    let ids: number[];
    if (selected && selectedKeys) {
      ids = Array.from(selectedKeys);
    } else {
      ids = [key];
      onSelectionChange?.(new Set([key]));
    }
    setMenu({ x: e.clientX, y: e.clientY, ids });
  }

  return (
    <Fragment>
      <table className="explorer-table">
      <thead>
        <tr>
          <SortableHeader label={t("common.white")} column="white" sort={sort} onSortChange={onSortChange} />
          <th>{t("gamesTable.whiteElo")}</th>
          <SortableHeader label={t("common.black")} column="black" sort={sort} onSortChange={onSortChange} />
          <th>{t("gamesTable.blackElo")}</th>
          <SortableHeader label={t("common.date")} column="date" sort={sort} onSortChange={onSortChange} />
          <th>{t("common.result")}</th>
          <th>{t("common.event")}</th>
          {(onDelete || onMove) && <th></th>}
        </tr>
      </thead>
      <tbody>
        {games.map((g, index) => {
          const selected = selectedKeys?.has(g.key) ?? false;
          return (
            <tr
              key={g.key}
              className={selected ? "explorer-table-row--selected" : ""}
              draggable={!!sourcePath}
              onDragStart={(e) => handleDragStart(e, g.key, selected)}
              onClick={(e) => handleRowClick(e, g.key, index)}
              onDoubleClick={() => onOpen(g.key)}
              onContextMenu={(e) => handleContextMenu(e, g.key, selected)}
            >
              <td>{g.white}</td>
              <td>{g.whiteElo ?? ""}</td>
              <td>{g.black}</td>
              <td>{g.blackElo ?? ""}</td>
              <td>{g.date}</td>
              <td>{g.result}</td>
              <td>{g.event}</td>
              {(onDelete || onMove) && (
                <td>
                  {onMove && (
                    <button
                      className="games-table-move"
                      title={t("gamesTable.moveGameButton")}
                      onClick={(e) => {
                        e.stopPropagation();
                        onMove(g.key);
                      }}
                    >
                      ➜
                    </button>
                  )}
                  {onDelete && (
                    <button
                      className="games-table-delete"
                      title={t("gamesTable.deleteGame")}
                      onClick={(e) => {
                        e.stopPropagation();
                        onDelete(g.key);
                      }}
                    >
                      🗑
                    </button>
                  )}
                </td>
              )}
            </tr>
          );
        })}
      </tbody>
    </table>
    {menu && buildContextMenuItems && (
      <ContextMenu x={menu.x} y={menu.y} items={buildContextMenuItems(menu.ids)} onClose={() => setMenu(null)} />
    )}
    </Fragment>
  );
}
