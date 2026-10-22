import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { useAutoDismissError } from "../useAutoDismissError";
import type { UpdatePreview, UpdatePreviewTournament } from "../types";

interface ExternalUpdatesReviewDialogProps {
  preview: UpdatePreview;
  onConfirm: (pgnPath: string) => void;
  onCancel: () => void;
}

// "AAAA.MM.JJ" brut (meme convention que HomeDockPanels::formatTournamentDateRange,
// pas de reformatage) -- vide si aucune partie retenue pour ce tournoi n'a de
// date exploitable (voir db::is_known_date cote Rust).
function formatDateRange(tour: UpdatePreviewTournament): string {
  if (!tour.dateStart) return tour.dateEnd ?? "";
  if (!tour.dateEnd || tour.dateEnd === tour.dateStart) return tour.dateStart;
  return `${tour.dateStart} – ${tour.dateEnd}`;
}

// Affiche le resultat de check_external_updates (deja telecharge/consolide/
// dedoublonne cote Rust, voir external_updates::compute_update_preview) avant
// import effectif -- aucune ecriture en base n'a encore eu lieu a ce stade,
// voir la discussion qui a mene a cette fonctionnalite ("aucune maj ne se
// fait sans l'accord de l'utilisateur"). Chaque tournoi peut etre decoche
// (rapporte par l'utilisateur comme manquant) : si au moins un l'est,
// filter_external_update_pgn reecrit un PGN sans les tournois exclus avant
// de rendre la main a onConfirm, plutot que d'importer puis de supprimer.
export function ExternalUpdatesReviewDialog({ preview, onConfirm, onCancel }: ExternalUpdatesReviewDialogProps) {
  const { t } = useTranslation();
  const [excluded, setExcluded] = useState<Set<string>>(new Set());
  const [filtering, setFiltering] = useState(false);
  const [error, setError] = useAutoDismissError();

  const toggle = (event: string) => {
    setExcluded((prev) => {
      const next = new Set(prev);
      if (next.has(event)) next.delete(event);
      else next.add(event);
      return next;
    });
  };

  const selectedTotal = useMemo(
    () => preview.tournaments.filter((tour) => !excluded.has(tour.event)).reduce((sum, tour) => sum + tour.games, 0),
    [preview.tournaments, excluded]
  );

  const handleConfirm = async () => {
    if (excluded.size === 0) {
      onConfirm(preview.pgnPath);
      return;
    }
    setFiltering(true);
    try {
      const filteredPath = await invoke<string>("filter_external_update_pgn", {
        pgnPath: preview.pgnPath,
        excludedEvents: Array.from(excluded),
      });
      onConfirm(filteredPath);
    } catch (e) {
      setFiltering(false);
      setError(String(e));
    }
  };

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{t("home.externalUpdates.title")}</h2>
        <p>
          {t("home.externalUpdates.reviewSummary", {
            total: preview.totalGames,
            from: preview.fromIssue,
            to: preview.toIssue,
          })}
        </p>
        {error && <p className="error">{error}</p>}
        <div className="modal-table-wrap">
          <table className="external-updates-table">
            <thead>
              <tr>
                <th></th>
                <th>{t("home.externalUpdates.columnEvent")}</th>
                <th>{t("home.externalUpdates.columnDates")}</th>
                <th>{t("home.externalUpdates.columnGames")}</th>
                <th>{t("home.externalUpdates.columnSource")}</th>
              </tr>
            </thead>
            <tbody>
              {preview.tournaments.map((tour) => (
                <tr key={tour.event}>
                  <td>
                    <input type="checkbox" checked={!excluded.has(tour.event)} onChange={() => toggle(tour.event)} />
                  </td>
                  <td>{tour.event}</td>
                  <td>{formatDateRange(tour)}</td>
                  <td>{tour.games}</td>
                  <td>
                    <span className={`external-updates-source-badge external-updates-source-badge--${tour.source}`}>
                      {t(`home.externalUpdates.source.${tour.source}`)}
                    </span>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <div className="modal-actions">
          <button onClick={onCancel}>{t("common.cancel")}</button>
          <button disabled={filtering || selectedTotal === 0} onClick={handleConfirm}>
            {filtering ? t("home.externalUpdates.filtering") : t("home.externalUpdates.importButton", { count: selectedTotal })}
          </button>
        </div>
      </div>
    </div>
  );
}
