import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useHomeContext } from "./HomeContext";
import { BasesTable } from "./components/BasesTable";
import { ContextMenu, type ContextMenuItem } from "./components/ContextMenu";
import { GamesTable, type GameRow } from "./components/GamesTable";
import { PageNav } from "./components/PageNav";

const GAMES_PAGE_SIZE = 200;
const PLAYERS_PAGE_SIZE = 200;

export function BasesPanelView() {
  const { t } = useTranslation();
  const { bases, activeSource, onSelectBase, onCreateBase, onAddExistingBase, onDropGamesOnBase, error } =
    useHomeContext();
  const selectedPath = activeSource?.kind === "base" ? activeSource.base.path : null;
  return (
    <div className="dock-panel-content">
      {error && <p className="error">{error}</p>}
      <div className="home-toolbar">
        <h2>{t("home.bases")}</h2>
        <button onClick={onCreateBase}>{t("home.newBase")}</button>
        <button onClick={onAddExistingBase}>{t("home.openExistingBase")}</button>
      </div>
      <BasesTable bases={bases} selectedPath={selectedPath} onSelect={onSelectBase} onDropGames={onDropGamesOnBase} />
    </div>
  );
}

export function PgnPanelView() {
  const { t } = useTranslation();
  const { recentPgns, activeSource, onSelectPgn, onImportClick } = useHomeContext();
  const selectedPath = activeSource?.kind === "pgn" ? activeSource.pgn.path : null;
  return (
    <div className="dock-panel-content">
      <div className="home-toolbar">
        <h2>{t("home.recentPgns")}</h2>
        <button onClick={onImportClick}>{t("home.importPgnFile")}</button>
      </div>
      <BasesTable bases={recentPgns} selectedPath={selectedPath} onSelect={onSelectPgn} />
    </div>
  );
}

export function PartiesPanelView() {
  const { t } = useTranslation();
  const {
    activeSource,
    games,
    gamesTotal,
    gamesLoading,
    gamesPage,
    gamesSearchInput,
    onGamesSearchChange,
    onGamesPageChange,
    gamesSort,
    onGamesSortChange,
    gamesAdvancedOpen,
    onToggleGamesAdvanced,
    gamesAdvancedFilter,
    onGamesAdvancedFilterChange,
    onGamesFilterCommit,
    onOpenBaseGame,
    onDeleteGame,
    onDeleteGames,
    onMoveGame,
    onTransferGames,
    selectedGameIds,
    onGamesSelectionChange,
    onToggleReference,
    onDeleteBase,
    pgnGames,
    onOpenPgnGame,
  } = useHomeContext();

  if (!activeSource) {
    return (
      <div className="dock-panel-content">
        <p className="notation-empty">{t("home.selectBaseOrPgn")}</p>
      </div>
    );
  }

  if (activeSource.kind === "pgn") {
    const rows: GameRow[] = pgnGames.map((g) => ({
      key: g.index,
      white: g.white,
      black: g.black,
      date: g.date,
      result: g.result,
      event: g.event,
    }));
    return (
      <div className="dock-panel-content">
        <div className="home-toolbar">
          <h2>{activeSource.pgn.name}</h2>
        </div>
        <GamesTable games={rows} onOpen={onOpenPgnGame} />
      </div>
    );
  }

  const base = activeSource.base;
  const rows: GameRow[] = games.map((g) => ({
    key: g.id,
    white: g.white,
    black: g.black,
    date: g.date,
    result: g.result,
    event: g.event,
  }));

  function buildContextMenuItems(ids: number[]): ContextMenuItem[] {
    return [
      { label: t("gamesTable.ctxOpen"), onClick: () => onOpenBaseGame(ids[0]), disabled: ids.length !== 1 },
      { label: t("gamesTable.ctxMoveTo"), onClick: () => onTransferGames(ids, "move") },
      { label: t("gamesTable.ctxCopyTo"), onClick: () => onTransferGames(ids, "copy") },
      { label: t("gamesTable.ctxDelete"), onClick: () => onDeleteGames(ids), danger: true },
    ];
  }

  return (
    <div className="dock-panel-content">
      <div className="home-toolbar">
        <h2>
          {base.isReference && <span title={t("home.referenceBase")}>📚 </span>}
          {base.missing && <span title={t("home.fileMissing")}>⚠️ </span>}
          {base.name}
        </h2>
        {!base.missing && (
          <button onClick={onToggleReference}>
            {base.isReference ? t("home.unmarkReference") : t("home.markReference")}
          </button>
        )}
        <button onClick={onDeleteBase}>{t("common.delete")}</button>
      </div>
      {!base.missing && (
        <div className="games-pagination">
          {!gamesAdvancedOpen && (
            <input
              type="search"
              placeholder={t("home.searchNamePlaceholder")}
              value={gamesSearchInput}
              onChange={(e) => onGamesSearchChange(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") onGamesFilterCommit();
              }}
            />
          )}
          {!gamesAdvancedOpen && (
            <button disabled={gamesLoading} onClick={onGamesFilterCommit}>
              {t("home.search")}
            </button>
          )}
          <button className="games-advanced-toggle" onClick={onToggleGamesAdvanced}>
            {t("home.advancedFilters")} {gamesAdvancedOpen ? "▲" : "▼"}
          </button>
          <PageNav
            page={gamesPage}
            pageSize={GAMES_PAGE_SIZE}
            total={gamesTotal}
            loading={gamesLoading}
            onPageChange={onGamesPageChange}
            emptyLabel={t("home.noGames")}
          />
        </div>
      )}
      {!base.missing && gamesAdvancedOpen && (
        <div className="games-advanced-filters">
          <input
            type="text"
            placeholder={t("home.whitePlaceholder")}
            value={gamesAdvancedFilter.white}
            onChange={(e) => onGamesAdvancedFilterChange({ white: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter") onGamesFilterCommit();
            }}
          />
          <input
            type="text"
            placeholder={t("home.blackPlaceholder")}
            value={gamesAdvancedFilter.black}
            onChange={(e) => onGamesAdvancedFilterChange({ black: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter") onGamesFilterCommit();
            }}
          />
          <select
            value={gamesAdvancedFilter.result}
            onChange={(e) => onGamesAdvancedFilterChange({ result: e.target.value })}
          >
            <option value="">{t("home.allResults")}</option>
            <option value="1-0">1-0</option>
            <option value="0-1">0-1</option>
            <option value="1/2-1/2">1/2-1/2</option>
            <option value="*">*</option>
          </select>
          <label>
            {t("home.dateFrom")}{" "}
            <input
              type="text"
              placeholder={t("home.datePlaceholder")}
              value={gamesAdvancedFilter.dateFrom}
              onChange={(e) => onGamesAdvancedFilterChange({ dateFrom: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") onGamesFilterCommit();
              }}
            />
          </label>
          <label>
            {t("home.dateTo")}{" "}
            <input
              type="text"
              placeholder={t("home.datePlaceholder")}
              value={gamesAdvancedFilter.dateTo}
              onChange={(e) => onGamesAdvancedFilterChange({ dateTo: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") onGamesFilterCommit();
              }}
            />
          </label>
          <input
            type="text"
            placeholder={t("home.timeControlPlaceholder")}
            value={gamesAdvancedFilter.timeControl}
            onChange={(e) => onGamesAdvancedFilterChange({ timeControl: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter") onGamesFilterCommit();
            }}
          />
          <button disabled={gamesLoading} onClick={onGamesFilterCommit}>
            {t("home.search")}
          </button>
        </div>
      )}
      <GamesTable
        games={rows}
        onOpen={onOpenBaseGame}
        onDelete={onDeleteGame}
        onMove={onMoveGame}
        sort={gamesSort}
        onSortChange={onGamesSortChange}
        sourcePath={base.path}
        selectedKeys={selectedGameIds}
        onSelectionChange={onGamesSelectionChange}
        buildContextMenuItems={buildContextMenuItems}
      />
    </div>
  );
}

export function PlayersPanelView() {
  const { t } = useTranslation();
  const {
    activeSource,
    players,
    playersTotal,
    playersLoading,
    playersPage,
    playersSearchInput,
    onPlayersSearchChange,
    onPlayersPageChange,
    onPlayersFilterCommit,
    onSelectPlayer,
    playersSort,
    onPlayersSortChange,
    onPrepareAgainstPlayer,
  } = useHomeContext();
  const [menu, setMenu] = useState<{ x: number; y: number; name: string } | null>(null);

  if (activeSource?.kind !== "base") {
    return (
      <div className="dock-panel-content">
        <p className="notation-empty">{t("home.selectBaseOrPgn")}</p>
      </div>
    );
  }

  return (
    <div className="dock-panel-content">
      <div className="games-pagination">
        <input
          type="search"
          placeholder={t("home.searchNamePlaceholder")}
          value={playersSearchInput}
          onChange={(e) => onPlayersSearchChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onPlayersFilterCommit();
          }}
        />
        <button disabled={playersLoading} onClick={onPlayersFilterCommit}>
          {t("home.search")}
        </button>
        <PageNav
          page={playersPage}
          pageSize={PLAYERS_PAGE_SIZE}
          total={playersTotal}
          loading={playersLoading}
          onPageChange={onPlayersPageChange}
          emptyLabel={t("home.noGames")}
        />
      </div>
      {players.length === 0 ? (
        <p className="notation-empty">{t("home.noPlayers")}</p>
      ) : (
        <table className="explorer-table">
          <thead>
            <tr>
              <th className="games-table-sortable" onClick={() => onPlayersSortChange("name")}>
                {t("home.playerName")} {playersSort?.column === "name" ? (playersSort.dir === "asc" ? "▲" : "▼") : ""}
              </th>
              <th className="games-table-sortable" onClick={() => onPlayersSortChange("games")}>
                {t("explorer.games")} {playersSort?.column === "games" ? (playersSort.dir === "asc" ? "▲" : "▼") : ""}
              </th>
            </tr>
          </thead>
          <tbody>
            {players.map((p) => (
              <tr
                key={p.name}
                onClick={() => onSelectPlayer(p.name)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setMenu({ x: e.clientX, y: e.clientY, name: p.name });
                }}
              >
                <td>{p.name}</td>
                <td>{p.games}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {menu && (
        <ContextMenu
          x={menu.x}
          y={menu.y}
          onClose={() => setMenu(null)}
          items={[
            {
              label: t("home.prepareAgainstWhite"),
              onClick: () => onPrepareAgainstPlayer(menu.name, true),
            },
            {
              label: t("home.prepareAgainstBlack"),
              onClick: () => onPrepareAgainstPlayer(menu.name, false),
            },
          ]}
        />
      )}
    </div>
  );
}

export const homeDockviewComponents = {
  bases: BasesPanelView,
  pgn: PgnPanelView,
  parties: PartiesPanelView,
  players: PlayersPanelView,
};
