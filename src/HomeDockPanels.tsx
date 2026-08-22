import { useHomeContext } from "./HomeContext";
import { BasesTable } from "./components/BasesTable";
import { GamesTable, type GameRow } from "./components/GamesTable";

const GAMES_PAGE_SIZE = 200;

export function BasesPanelView() {
  const { bases, activeSource, onSelectBase, onCreateBase, onAddExistingBase, error } = useHomeContext();
  const selectedPath = activeSource?.kind === "base" ? activeSource.base.path : null;
  return (
    <div className="dock-panel-content">
      {error && <p className="error">{error}</p>}
      <div className="home-toolbar">
        <h2>Bases</h2>
        <button onClick={onCreateBase}>Nouvelle base</button>
        <button onClick={onAddExistingBase}>Ouvrir une base existante</button>
      </div>
      <BasesTable bases={bases} selectedPath={selectedPath} onSelect={onSelectBase} />
    </div>
  );
}

export function PgnPanelView() {
  const { recentPgns, activeSource, onSelectPgn, onImportClick } = useHomeContext();
  const selectedPath = activeSource?.kind === "pgn" ? activeSource.pgn.path : null;
  return (
    <div className="dock-panel-content">
      <div className="home-toolbar">
        <h2>Fichiers PGN récents</h2>
        <button onClick={onImportClick}>Importer un fichier PGN...</button>
      </div>
      <BasesTable bases={recentPgns} selectedPath={selectedPath} onSelect={onSelectPgn} />
    </div>
  );
}

export function PartiesPanelView() {
  const {
    activeSource,
    games,
    gamesTotal,
    gamesPage,
    gamesSearchInput,
    onGamesSearchChange,
    onGamesPageChange,
    onOpenBaseGame,
    onDeleteGame,
    onToggleReference,
    onDeleteBase,
    pgnGames,
    onOpenPgnGame,
  } = useHomeContext();

  if (!activeSource) {
    return (
      <div className="dock-panel-content">
        <p className="notation-empty">Sélectionne une base ou un fichier PGN pour voir ses parties.</p>
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
  return (
    <div className="dock-panel-content">
      <div className="home-toolbar">
        <h2>
          {base.isReference && <span title="Base de référence">📚 </span>}
          {base.missing && <span title="Fichier introuvable">⚠️ </span>}
          {base.name}
        </h2>
        {!base.missing && (
          <button onClick={onToggleReference}>
            {base.isReference ? "Retirer le marquage référence" : "Marquer comme référence"}
          </button>
        )}
        <button onClick={onDeleteBase}>Supprimer</button>
      </div>
      {!base.missing && (
        <div className="games-pagination">
          <input
            type="search"
            placeholder="Rechercher Blancs/Noirs..."
            value={gamesSearchInput}
            onChange={(e) => onGamesSearchChange(e.target.value)}
          />
          <span>
            {gamesTotal === 0
              ? "Aucune partie"
              : `${gamesPage * GAMES_PAGE_SIZE + 1}–${Math.min(
                  (gamesPage + 1) * GAMES_PAGE_SIZE,
                  gamesTotal
                )} sur ${gamesTotal}`}
          </span>
          <button disabled={gamesPage === 0} onClick={() => onGamesPageChange(gamesPage - 1)}>
            ← Précédent
          </button>
          <button
            disabled={(gamesPage + 1) * GAMES_PAGE_SIZE >= gamesTotal}
            onClick={() => onGamesPageChange(gamesPage + 1)}
          >
            Suivant →
          </button>
        </div>
      )}
      <GamesTable games={rows} onOpen={onOpenBaseGame} onDelete={onDeleteGame} />
    </div>
  );
}

export const homeDockviewComponents = {
  bases: BasesPanelView,
  pgn: PgnPanelView,
  parties: PartiesPanelView,
};
