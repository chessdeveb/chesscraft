import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { useHomeContext } from "./HomeContext";
import { BasesTable } from "./components/BasesTable";
import { ContextMenu, type ContextMenuItem } from "./components/ContextMenu";
import { GamesTable, type GameRow } from "./components/GamesTable";
import { PageNav } from "./components/PageNav";
import { SelectionPerformance } from "./components/SelectionPerformance";
import type { TournamentSummary } from "./types";
import { ColumnResizeHandle, useColumnWidths } from "./useColumnWidths";

const PLAYERS_TABLE_COLUMNS = [
  { key: "name", default: 220 },
  { key: "games", default: 80 },
];

const TOURNAMENTS_TABLE_COLUMNS = [
  { key: "name", default: 220 },
  { key: "site", default: 100 },
  { key: "timeControl", default: 120 },
  { key: "dates", default: 150 },
  { key: "games", default: 80 },
];

// "AAAA.MM.JJ" brut (meme convention que GamesTable, pas de reformatage) --
// vide si aucune partie du tournoi n'a de date exploitable, une seule date
// si debut et fin coincident (tournoi d'un jour ou rattrapage partiel).
function formatTournamentDateRange(tour: TournamentSummary): string {
  if (!tour.dateStart) return tour.dateEnd ?? "";
  if (!tour.dateEnd || tour.dateEnd === tour.dateStart) return tour.dateStart;
  return `${tour.dateStart} – ${tour.dateEnd}`;
}

const FIDE_TABLE_COLUMNS = [
  { key: "rowNumber", default: 70 },
  { key: "name", default: 200 },
  { key: "fed", default: 70 },
  { key: "sex", default: 50 },
  { key: "title", default: 60 },
  { key: "wTitle", default: 60 },
  { key: "oTitle", default: 60 },
  { key: "std", default: 100 },
  { key: "rapid", default: 90 },
  { key: "blitz", default: 80 },
  { key: "birthYear", default: 70 },
  { key: "flag", default: 90 },
];

const GAMES_PAGE_SIZE = 200;
const PLAYERS_PAGE_SIZE = 200;
const FIDE_PAGE_SIZE = 200;
const TOURNAMENTS_PAGE_SIZE = 200;

export function BasesPanelView() {
  const { t } = useTranslation();
  const {
    bases,
    activeSource,
    onSelectBase,
    onCreateBase,
    onAddExistingBase,
    onDropGamesOnBase,
    onToggleReference,
    onDeleteBase,
    error,
    externalUpdatesEstimate,
    externalUpdatesBusy,
    onCheckExternalUpdates,
  } = useHomeContext();
  const selectedBase = activeSource?.kind === "base" ? activeSource.base : null;
  return (
    <div className="dock-panel-content">
      {error && <p className="error">{error}</p>}
      <div className="home-toolbar">
        <h2>{t("home.bases")}</h2>
        <button className="icon-button" onClick={onCreateBase} title={t("home.newBase")} aria-label={t("home.newBase")}>
          ➕
        </button>
        <button className="icon-button" onClick={onAddExistingBase} title={t("home.openExistingBase")} aria-label={t("home.openExistingBase")}>
          📂
        </button>
        {selectedBase && (
          <>
            {!selectedBase.missing && (
              <button
                className="icon-button"
                onClick={onToggleReference}
                title={selectedBase.isReference ? t("home.unmarkReference") : t("home.markReference")}
                aria-label={selectedBase.isReference ? t("home.unmarkReference") : t("home.markReference")}
              >
                📚
              </button>
            )}
            <button className="icon-button" onClick={onDeleteBase} title={t("common.delete")} aria-label={t("common.delete")}>
              🗑️
            </button>
          </>
        )}
        <button
          className="icon-button"
          onClick={onCheckExternalUpdates}
          disabled={externalUpdatesBusy !== null}
          title={
            externalUpdatesBusy === "checking"
              ? t("home.externalUpdates.checking")
              : externalUpdatesBusy === "estimating"
                ? t("home.externalUpdates.estimating")
                : t("home.externalUpdates.button")
          }
          aria-label={t("home.externalUpdates.button")}
        >
          🔄
        </button>
        {externalUpdatesBusy === null && externalUpdatesEstimate && (
          <span className="external-updates-badge" title={t("home.externalUpdates.badgeTitle")}>
            {t("home.externalUpdates.weeksBehind", { weeks: externalUpdatesEstimate.weeksBehind })}
          </span>
        )}
      </div>
      <BasesTable bases={bases} selectedPath={selectedBase?.path ?? null} onSelect={onSelectBase} onDropGames={onDropGamesOnBase} />
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
      whiteElo: g.whiteElo,
      blackElo: g.blackElo,
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
    whiteElo: g.whiteElo,
    blackElo: g.blackElo,
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
            placeholder={t("home.tournamentPlaceholder")}
            value={gamesAdvancedFilter.tournament}
            onChange={(e) => onGamesAdvancedFilterChange({ tournament: e.target.value })}
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
      <SelectionPerformance basePath={base.path} selectedIds={selectedGameIds} />
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
  const { widths, startResize, totalWidth } = useColumnWidths("playersTable", PLAYERS_TABLE_COLUMNS);

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
        <div className="resizable-table-wrap">
        <table className="explorer-table resizable-table" style={{ width: totalWidth }}>
          <thead>
            <tr>
              <th className="games-table-sortable" style={{ width: widths.name }} onClick={() => onPlayersSortChange("name")}>
                {t("home.playerName")} {playersSort?.column === "name" ? (playersSort.dir === "asc" ? "▲" : "▼") : ""}
                <ColumnResizeHandle onMouseDown={startResize("name")} />
              </th>
              <th className="games-table-sortable" style={{ width: widths.games }} onClick={() => onPlayersSortChange("games")}>
                {t("explorer.games")} {playersSort?.column === "games" ? (playersSort.dir === "asc" ? "▲" : "▼") : ""}
                <ColumnResizeHandle onMouseDown={startResize("games")} />
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
        </div>
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

// Cellule de cadence editable d'un tournoi : un simple champ texte (meme
// format PGN TimeControl que la bulle d'aide de SaveBasePicker, reutilisee ici
// a l'identique) plutot que les trois boutons Blitz/Rapide/Classique de ce
// dernier -- ici on n'edite jamais qu'un seul champ a la fois, pas besoin du
// raccourci de saisie, seulement de voir/corriger la valeur deja enregistree.
function TournamentCadenceCell({ tournament }: { tournament: TournamentSummary }) {
  const { t } = useTranslation();
  const { onSetTournamentCadence } = useHomeContext();
  const [value, setValue] = useState(tournament.timeControl ?? "");
  useEffect(() => setValue(tournament.timeControl ?? ""), [tournament.id, tournament.timeControl]);

  function commit() {
    if (value !== (tournament.timeControl ?? "")) onSetTournamentCadence(tournament.id, value);
  }

  return (
    <input
      type="text"
      className="tournament-cadence-input"
      value={value}
      title={t("savePicker.timeControlHelp")}
      placeholder={t("savePicker.timeControlPlaceholder")}
      onClick={(e) => e.stopPropagation()}
      onChange={(e) => setValue(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
      }}
    />
  );
}

export function TournamentsPanelView() {
  const { t } = useTranslation();
  const {
    activeSource,
    tournaments,
    tournamentsTotal,
    tournamentsLoading,
    tournamentsPage,
    tournamentsSearchInput,
    onTournamentsSearchChange,
    onTournamentsPageChange,
    onTournamentsFilterCommit,
    tournamentsSort,
    onTournamentsSortChange,
    onFilterGamesByTournament,
  } = useHomeContext();
  const { widths, startResize, totalWidth } = useColumnWidths("tournamentsTable", TOURNAMENTS_TABLE_COLUMNS);

  // Au survol : les details du tournoi meme si le nom/lieu est tronque par
  // une colonne trop etroite -- pas de nouvelle donnee, juste ce qui est deja
  // dans la ligne (voir TournamentSummary).
  function tournamentTooltip(tour: TournamentSummary): string {
    return [
      `${t("home.tournamentName")}: ${tour.name}`,
      `${t("home.tournamentSite")}: ${tour.site}`,
      `${t("savePicker.timeControl")}: ${tour.timeControl || "?"}`,
      `${t("home.tournamentDates")}: ${formatTournamentDateRange(tour) || "?"}`,
      `${t("explorer.games")}: ${tour.games}`,
    ].join("\n");
  }

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
          value={tournamentsSearchInput}
          onChange={(e) => onTournamentsSearchChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") onTournamentsFilterCommit();
          }}
        />
        <button disabled={tournamentsLoading} onClick={onTournamentsFilterCommit}>
          {t("home.search")}
        </button>
        <PageNav
          page={tournamentsPage}
          pageSize={TOURNAMENTS_PAGE_SIZE}
          total={tournamentsTotal}
          loading={tournamentsLoading}
          onPageChange={onTournamentsPageChange}
          emptyLabel={t("home.noGames")}
        />
      </div>
      {tournaments.length === 0 ? (
        <p className="notation-empty">{t("home.noTournaments")}</p>
      ) : (
        <div className="resizable-table-wrap">
        <table className="explorer-table resizable-table" style={{ width: totalWidth }}>
          <thead>
            <tr>
              <th className="games-table-sortable" style={{ width: widths.name }} onClick={() => onTournamentsSortChange("name")}>
                {t("home.tournamentName")} {tournamentsSort?.column === "name" ? (tournamentsSort.dir === "asc" ? "▲" : "▼") : ""}
                <ColumnResizeHandle onMouseDown={startResize("name")} />
              </th>
              <th style={{ width: widths.site }}>
                {t("home.tournamentSite")}
                <ColumnResizeHandle onMouseDown={startResize("site")} />
              </th>
              <th style={{ width: widths.timeControl }}>
                {t("savePicker.timeControl")}
                <ColumnResizeHandle onMouseDown={startResize("timeControl")} />
              </th>
              <th style={{ width: widths.dates }}>
                {t("home.tournamentDates")}
                <ColumnResizeHandle onMouseDown={startResize("dates")} />
              </th>
              <th className="games-table-sortable" style={{ width: widths.games }} onClick={() => onTournamentsSortChange("games")}>
                {t("explorer.games")} {tournamentsSort?.column === "games" ? (tournamentsSort.dir === "asc" ? "▲" : "▼") : ""}
                <ColumnResizeHandle onMouseDown={startResize("games")} />
              </th>
            </tr>
          </thead>
          <tbody>
            {tournaments.map((tour) => (
              <tr
                key={tour.id}
                className="tournament-row"
                title={tournamentTooltip(tour)}
                onClick={() => onFilterGamesByTournament(tour.name)}
              >
                <td>{tour.name}</td>
                <td>{tour.site}</td>
                <td>
                  <TournamentCadenceCell tournament={tour} />
                </td>
                <td>{formatTournamentDateRange(tour)}</td>
                <td>{tour.games}</td>
              </tr>
            ))}
          </tbody>
        </table>
        </div>
      )}
    </div>
  );
}

function formatLastUpdated(iso: string | null, t: (key: string, opts?: Record<string, unknown>) => string): string {
  if (!iso) return t("home.fideNeverUpdated");
  const seconds = Number(iso);
  if (Number.isNaN(seconds)) return t("home.fideNeverUpdated");
  const date = new Date(seconds * 1000);
  return t("home.fideLastUpdated", { date: date.toLocaleDateString() });
}

export function FidePanelView() {
  const { t } = useTranslation();
  const {
    fidePlayers,
    fideTotal,
    fideLoading,
    fidePage,
    fideSearchInput,
    onFideSearchChange,
    onFidePageChange,
    onFideFilterCommit,
    fideAdvancedOpen,
    onToggleFideAdvanced,
    fideAdvancedFilter,
    onFideAdvancedFilterChange,
    fideSort,
    onFideSortChange,
    fideLastUpdated,
    fideRefreshing,
    fideRefreshProgress,
    fideRefreshError,
    onFideRefresh,
  } = useHomeContext();

  const sortIndicator = (column: string) => (fideSort?.column === column ? (fideSort.dir === "asc" ? "▲" : "▼") : "");
  const { widths, startResize, totalWidth } = useColumnWidths("fideTable", FIDE_TABLE_COLUMNS);

  // Cette table vient directement de la base FIDE : chaque ligne a deja son
  // vrai fideId, pas besoin de recherche par nom (contrairement a l'en-tete
  // de la fenetre Partie, ou le fideId n'est connu que si le PGN le porte).
  const openFidePage = (fideId: string) => {
    openUrl(`https://ratings.fide.com/profile/${encodeURIComponent(fideId)}`).catch((e) =>
      console.error("openFidePage", e)
    );
  };

  return (
    <div className="dock-panel-content">
      <div className="home-toolbar">
        <span className="fide-status">{formatLastUpdated(fideLastUpdated, t)}</span>
        <button onClick={onFideRefresh} disabled={fideRefreshing}>
          {fideRefreshing ? t("home.fideRefreshing", { count: fideRefreshProgress.toLocaleString() }) : t("home.fideRefresh")}
        </button>
      </div>
      {fideRefreshError && <p className="error">{fideRefreshError}</p>}
      <div className="games-pagination">
        {!fideAdvancedOpen && (
          <input
            type="search"
            placeholder={t("home.searchNamePlaceholder")}
            value={fideSearchInput}
            onChange={(e) => onFideSearchChange(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") onFideFilterCommit();
            }}
          />
        )}
        {!fideAdvancedOpen && (
          <button disabled={fideLoading} onClick={onFideFilterCommit}>
            {t("home.search")}
          </button>
        )}
        <button className="games-advanced-toggle" onClick={onToggleFideAdvanced}>
          {t("home.advancedFilters")} {fideAdvancedOpen ? "▲" : "▼"}
        </button>
        <PageNav
          page={fidePage}
          pageSize={FIDE_PAGE_SIZE}
          total={fideTotal}
          loading={fideLoading}
          onPageChange={onFidePageChange}
          emptyLabel={t("home.noGames")}
        />
      </div>
      {fideAdvancedOpen && (
        <div className="games-advanced-filters">
          <input
            type="text"
            placeholder={t("home.fideFedPlaceholder")}
            value={fideAdvancedFilter.fed}
            onChange={(e) => onFideAdvancedFilterChange({ fed: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter") onFideFilterCommit();
            }}
          />
          <select value={fideAdvancedFilter.title} onChange={(e) => onFideAdvancedFilterChange({ title: e.target.value })}>
            <option value="">{t("home.allTitles")}</option>
            <option value="GM">GM</option>
            <option value="IM">IM</option>
            <option value="FM">FM</option>
            <option value="CM">CM</option>
            <option value="WGM">WGM</option>
            <option value="WIM">WIM</option>
            <option value="WFM">WFM</option>
            <option value="WCM">WCM</option>
            <option value="NM">NM</option>
          </select>
          <select value={fideAdvancedFilter.sex} onChange={(e) => onFideAdvancedFilterChange({ sex: e.target.value })}>
            <option value="">{t("home.allSexes")}</option>
            <option value="M">M</option>
            <option value="F">F</option>
          </select>
          <select value={fideAdvancedFilter.status} onChange={(e) => onFideAdvancedFilterChange({ status: e.target.value })}>
            <option value="">{t("home.allStatuses")}</option>
            <option value="active">{t("home.fideActive")}</option>
            <option value="inactive">{t("home.fideInactive")}</option>
          </select>
          <label>
            {t("home.fideBirthYearFrom")}{" "}
            <input
              type="text"
              inputMode="numeric"
              placeholder={t("home.fideBirthYearPlaceholder")}
              value={fideAdvancedFilter.birthYearMin}
              onChange={(e) => onFideAdvancedFilterChange({ birthYearMin: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") onFideFilterCommit();
              }}
            />
          </label>
          <label>
            {t("home.fideBirthYearTo")}{" "}
            <input
              type="text"
              inputMode="numeric"
              placeholder={t("home.fideBirthYearPlaceholder")}
              value={fideAdvancedFilter.birthYearMax}
              onChange={(e) => onFideAdvancedFilterChange({ birthYearMax: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Enter") onFideFilterCommit();
              }}
            />
          </label>
          <button disabled={fideLoading} onClick={onFideFilterCommit}>
            {t("home.search")}
          </button>
        </div>
      )}
      {fidePlayers.length === 0 ? (
        <p className="notation-empty">{t("home.noPlayers")}</p>
      ) : (
        <div className="fide-table-wrap">
          <table className="explorer-table resizable-table" style={{ width: totalWidth }}>
            <thead>
              <tr>
                <th style={{ width: widths.rowNumber }}>
                  {t("home.rowNumber")}
                  <ColumnResizeHandle onMouseDown={startResize("rowNumber")} />
                </th>
                <th className="games-table-sortable" style={{ width: widths.name }} onClick={() => onFideSortChange("name")}>
                  {t("home.playerName")} {sortIndicator("name")}
                  <ColumnResizeHandle onMouseDown={startResize("name")} />
                </th>
                <th className="games-table-sortable" style={{ width: widths.fed }} onClick={() => onFideSortChange("fed")}>
                  {t("home.fideFed")} {sortIndicator("fed")}
                  <ColumnResizeHandle onMouseDown={startResize("fed")} />
                </th>
                <th style={{ width: widths.sex }}>
                  {t("home.fideSex")}
                  <ColumnResizeHandle onMouseDown={startResize("sex")} />
                </th>
                <th style={{ width: widths.title }}>
                  {t("home.fideTitle")}
                  <ColumnResizeHandle onMouseDown={startResize("title")} />
                </th>
                <th style={{ width: widths.wTitle }}>
                  {t("home.fideWTitle")}
                  <ColumnResizeHandle onMouseDown={startResize("wTitle")} />
                </th>
                <th style={{ width: widths.oTitle }}>
                  {t("home.fideOTitle")}
                  <ColumnResizeHandle onMouseDown={startResize("oTitle")} />
                </th>
                <th className="games-table-sortable" style={{ width: widths.std }} onClick={() => onFideSortChange("std")}>
                  {t("home.fideStd")} {sortIndicator("std")}
                  <ColumnResizeHandle onMouseDown={startResize("std")} />
                </th>
                <th className="games-table-sortable" style={{ width: widths.rapid }} onClick={() => onFideSortChange("rapid")}>
                  {t("home.fideRapid")} {sortIndicator("rapid")}
                  <ColumnResizeHandle onMouseDown={startResize("rapid")} />
                </th>
                <th className="games-table-sortable" style={{ width: widths.blitz }} onClick={() => onFideSortChange("blitz")}>
                  {t("home.fideBlitz")} {sortIndicator("blitz")}
                  <ColumnResizeHandle onMouseDown={startResize("blitz")} />
                </th>
                <th
                  className="games-table-sortable"
                  style={{ width: widths.birthYear }}
                  onClick={() => onFideSortChange("birthYear")}
                >
                  {t("home.fideBirthYear")} {sortIndicator("birthYear")}
                  <ColumnResizeHandle onMouseDown={startResize("birthYear")} />
                </th>
                <th style={{ width: widths.flag }}>
                  {t("home.fideFlag")}
                  <ColumnResizeHandle onMouseDown={startResize("flag")} />
                </th>
              </tr>
            </thead>
            <tbody>
              {fidePlayers.map((p, index) => (
                <tr key={p.fideId}>
                  <td>{fidePage * FIDE_PAGE_SIZE + index + 1}</td>
                  <td>
                    <span className="player-link" title={t("notation.openFidePage")} onClick={() => openFidePage(p.fideId)}>
                      {p.name}
                    </span>
                  </td>
                  <td>{p.fed}</td>
                  <td>{p.sex}</td>
                  <td>{p.title}</td>
                  <td>{p.wTitle}</td>
                  <td>{p.oTitle}</td>
                  <td>{p.stdRating ?? ""}</td>
                  <td>{p.rapidRating ?? ""}</td>
                  <td>{p.blitzRating ?? ""}</td>
                  <td>{p.birthYear ?? ""}</td>
                  <td>{p.flag}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

export const homeDockviewComponents = {
  bases: BasesPanelView,
  pgn: PgnPanelView,
  parties: PartiesPanelView,
  players: PlayersPanelView,
  fide: FidePanelView,
  tournaments: TournamentsPanelView,
};
