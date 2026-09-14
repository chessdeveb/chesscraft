import { useState } from "react";
import { useTranslation } from "react-i18next";

interface GameReportDialogProps {
  maxThreads: number;
  onConfirm: (depth: number, threads: number) => void;
  onCancel: () => void;
}

// Choix de la profondeur/du nombre de coeurs avant de lancer un rapport de
// partie (analyse complete, coup par coup) -- memorise separement des
// reglages du panneau Moteur (cle localStorage dediee) : l'utilisateur veut
// typiquement une profondeur differente pour un rapport complet (potentiellement
// tres long, plusieurs dizaines de coups) que pour l'analyse interactive
// d'une seule position.
export function GameReportDialog({ maxThreads, onConfirm, onCancel }: GameReportDialogProps) {
  const { t } = useTranslation();
  const [depth, setDepth] = useState(() => Number(localStorage.getItem("gameReportDepth")) || 18);
  const [threads, setThreads] = useState(() => Math.min(Number(localStorage.getItem("gameReportThreads")) || 1, maxThreads));

  function handleConfirm() {
    localStorage.setItem("gameReportDepth", String(depth));
    localStorage.setItem("gameReportThreads", String(threads));
    onConfirm(depth, threads);
  }

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{t("gameReport.title")}</h2>
        <div className="engine-settings-row">
          <label className="engine-field">
            {t("engine.depth")}
            <select value={depth} onChange={(e) => setDepth(Number(e.currentTarget.value))}>
              <option value={10}>10</option>
              <option value={14}>14</option>
              <option value={18}>18</option>
              <option value={22}>22</option>
              <option value={26}>26</option>
            </select>
          </label>
          <label className="engine-field">
            {t("engine.threads")}
            <select value={threads} onChange={(e) => setThreads(Number(e.currentTarget.value))}>
              {Array.from({ length: maxThreads }, (_, i) => i + 1).map((n) => (
                <option key={n} value={n}>
                  {n}
                </option>
              ))}
            </select>
          </label>
        </div>
        <div className="modal-actions">
          <button onClick={onCancel}>{t("common.cancel")}</button>
          <button onClick={handleConfirm}>{t("gameReport.start")}</button>
        </div>
      </div>
    </div>
  );
}
