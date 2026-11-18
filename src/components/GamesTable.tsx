import { Fragment, useEffect, useRef, useState, type DragEvent, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { ContextMenu, type ContextMenuItem } from "./ContextMenu";
import { isEditableTarget } from "../annotationShortcuts";
import { GAME_DRAG_MIME } from "../dnd";
import type { GamesSort, GamesSortColumn } from "../HomeContext";
import { ColumnResizeHandle, useColumnWidths } from "../useColumnWidths";

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
  /** Alt+clic sur une ligne : apercu de la partie (voir GamePreview). Absent
      = pas d'apercu, Alt+clic se comporte comme un clic ordinaire. */
  onPreview?: (key: number, anchor: { x: number; y: number }) => void;
}

// Sous Windows, relacher Alt seul active la barre de menu native. Apres un
// Alt+clic (apercu), on empeche ce relachement-la d'y arriver : un seul
// keyup d'Alt, dans les secondes qui suivent.
function swallowNextAltKeyUp() {
  const stop = (e: KeyboardEvent) => {
    if (e.key === "Alt") e.preventDefault();
  };
  window.addEventListener("keyup", stop, true);
  setTimeout(() => window.removeEventListener("keyup", stop, true), 5000);
}

function SortableHeader({
  label,
  column,
  sort,
  onSortChange,
  width,
  onResizeStart,
}: {
  label: string;
  column: GamesSortColumn;
  sort?: GamesSort | null;
  onSortChange?: (column: GamesSortColumn) => void;
  width: number;
  onResizeStart: (e: MouseEvent) => void;
}) {
  const active = sort?.column === column;
  return (
    <th
      className={onSortChange ? "games-table-sortable" : undefined}
      style={{ width }}
      onClick={onSortChange ? () => onSortChange(column) : undefined}
    >
      {label} {onSortChange && active ? (sort!.dir === "asc" ? "▲" : "▼") : ""}
      <ColumnResizeHandle onMouseDown={onResizeStart} />
    </th>
  );
}

// Colonnes redimensionnables (voir useColumnWidths) ; toutes chiffrees
// explicitement, y compris Evenement -- une colonne sans largeur declaree
// faussait la repartition de table-layout:fixed (voir l'historique de
// .resizable-table, App.css). La colonne d'actions (boutons
// deplacer/supprimer, presente seulement si onDelete/onMove) garde sa
// largeur naturelle, minime, pas la peine de la rendre redimensionnable.
const GAMES_TABLE_COLUMNS = [
  { key: "white", default: 140 },
  { key: "whiteElo", default: 60 },
  { key: "black", default: 140 },
  { key: "blackElo", default: 60 },
  { key: "date", default: 90 },
  { key: "result", default: 64 },
  { key: "event", default: 180 },
];
// Colonne d'actions (boutons deplacer/supprimer) : largeur fixe, non
// redimensionnable, mais doit quand meme etre comptee dans la largeur totale
// de la table (voir totalWidth, useColumnWidths) -- sans ca, cette colonne
// non suivie par le hook se retrouverait ecrasee a 0.
const GAMES_TABLE_ACTIONS_WIDTH = 56;

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
  onPreview,
}: GamesTableProps) {
  const { t } = useTranslation();
  const lastIndexRef = useRef<number | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; ids: number[] } | null>(null);
  const { widths, startResize, totalWidth } = useColumnWidths("gamesTable", GAMES_TABLE_COLUMNS);
  const wrapRef = useRef<HTMLDivElement | null>(null);

  // Barre d'espace : meme apercu que l'Alt+clic, pour la ligne selectionnee
  // (la derniere cliquee si plusieurs le sont). Sans effet pendant une saisie
  // ou sur un bouton, et quand l'onglet est masque (ses lignes n'ont alors
  // aucune taille a l'ecran). Quand un apercu est deja ouvert, c'est lui qui
  // prend la barre d'espace (lecture/pause) avant ce ecouteur.
  useEffect(() => {
    if (!onPreview) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key !== " " || e.defaultPrevented || e.repeat || e.ctrlKey || e.altKey || e.metaKey || e.shiftKey) return;
      const target = e.target;
      if (isEditableTarget(target)) return;
      if (target instanceof HTMLElement && target.closest("button, a, summary, [role=button]")) return;
      if (!selectedKeys || selectedKeys.size === 0) return;
      const lastKey = lastIndexRef.current !== null ? games[lastIndexRef.current]?.key : undefined;
      const key = lastKey !== undefined && selectedKeys.has(lastKey) ? lastKey : selectedKeys.values().next().value;
      const row = wrapRef.current?.querySelector<HTMLElement>(`tr[data-game-key="${key}"]`);
      if (key === undefined || !row) return;
      row.scrollIntoView({ block: "nearest" });
      const rect = row.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) return;
      e.preventDefault();
      onPreview!(key, { x: rect.left + Math.min(rect.width / 2, 200), y: rect.bottom });
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onPreview, selectedKeys, games]);

  if (games.length === 0) {
    return <p className="notation-empty">{t("gamesTable.empty")}</p>;
  }

  function handleRowClick(e: MouseEvent, key: number, index: number) {
    // apercu : ne touche pas a la selection
    if (e.altKey && onPreview) {
      e.preventDefault();
      swallowNextAltKeyUp();
      onPreview(key, { x: e.clientX, y: e.clientY });
      return;
    }
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
      <div className="resizable-table-wrap" ref={wrapRef}>
      <table
        className="explorer-table resizable-table"
        style={{ width: totalWidth + (onDelete || onMove ? GAMES_TABLE_ACTIONS_WIDTH : 0) }}
      >
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
          <th style={{ width: widths.whiteElo }}>
            {t("gamesTable.whiteElo")}
            <ColumnResizeHandle onMouseDown={startResize("whiteElo")} />
          </th>
          <SortableHeader
            label={t("common.black")}
            column="black"
            sort={sort}
            onSortChange={onSortChange}
            width={widths.black}
            onResizeStart={startResize("black")}
          />
          <th style={{ width: widths.blackElo }}>
            {t("gamesTable.blackElo")}
            <ColumnResizeHandle onMouseDown={startResize("blackElo")} />
          </th>
          <SortableHeader
            label={t("common.date")}
            column="date"
            sort={sort}
            onSortChange={onSortChange}
            width={widths.date}
            onResizeStart={startResize("date")}
          />
          <th style={{ width: widths.result }}>
            {t("common.result")}
            <ColumnResizeHandle onMouseDown={startResize("result")} />
          </th>
          <th style={{ width: widths.event }}>
            {t("common.event")}
            <ColumnResizeHandle onMouseDown={startResize("event")} />
          </th>
          {(onDelete || onMove) && <th></th>}
        </tr>
      </thead>
      <tbody>
        {games.map((g, index) => {
          const selected = selectedKeys?.has(g.key) ?? false;
          return (
            <tr
              key={g.key}
              data-game-key={g.key}
              className={selected ? "explorer-table-row--selected" : ""}
              draggable={!!sourcePath}
              title={onPreview ? t("gamesTable.previewHint") : undefined}
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
    </div>
    {menu && buildContextMenuItems && (
      <ContextMenu x={menu.x} y={menu.y} items={buildContextMenuItems(menu.ids)} onClose={() => setMenu(null)} />
    )}
    </Fragment>
  );
}
