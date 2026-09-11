import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useTranslation } from "react-i18next";
import { BasesTable } from "./BasesTable";
import { useAutoDismissError } from "../useAutoDismissError";
import type { BaseInfo } from "../types";

interface ToolBasePickerProps {
  title: string;
  onConfirm: (path: string) => void;
  onCancel: () => void;
}

// Choix d'une base existante pour une operation de maintenance (menu Outils
// > Recalculer les stats/Dedoublonner/Resoudre les identifiants FIDE) --
// contrairement a ImportBasePicker/SaveBasePicker, pas de bouton "Nouvelle
// base" : ces operations n'ont de sens que sur une base deja peuplee.
export function ToolBasePicker({ title, onConfirm, onCancel }: ToolBasePickerProps) {
  const { t } = useTranslation();
  const [bases, setBases] = useState<BaseInfo[]>([]);
  const [basesLoading, setBasesLoading] = useState(true);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [error, setError] = useAutoDismissError();

  useEffect(() => {
    invoke<BaseInfo[]>("list_bases")
      .then(setBases)
      .catch((e) => setError(String(e)))
      .finally(() => setBasesLoading(false));
  }, []);

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{title}</h2>
        {error && <p className="error">{error}</p>}
        <div className="modal-table-wrap">
          <BasesTable bases={bases} loading={basesLoading} selectedPath={selectedPath} onSelect={(b) => setSelectedPath(b.path)} />
        </div>
        <div className="modal-actions">
          <button onClick={onCancel}>{t("common.cancel")}</button>
          <button disabled={!selectedPath} onClick={() => selectedPath && onConfirm(selectedPath)}>
            {t("tools.pickBaseConfirm")}
          </button>
        </div>
      </div>
    </div>
  );
}
