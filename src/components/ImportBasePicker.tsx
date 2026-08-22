import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { BasesTable } from "./BasesTable";
import type { BaseInfo } from "../types";

interface ImportBasePickerProps {
  pgnPath: string;
  onConfirm: (basePath: string) => void;
  onCancel: () => void;
}

function baseNameFromPath(path: string): string {
  const fileName = path.split(/[\\/]/).pop() ?? path;
  return fileName.replace(/\.sqlite$/i, "");
}

export function ImportBasePicker({ pgnPath, onConfirm, onCancel }: ImportBasePickerProps) {
  const [bases, setBases] = useState<BaseInfo[]>([]);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<BaseInfo[]>("list_bases")
      .then(setBases)
      .catch((e) => setError(String(e)));
  }, []);

  async function handleCreateBase() {
    try {
      const path = await save({ filters: [{ name: "Base d'échecs", extensions: ["sqlite"] }] });
      if (!path) return;
      const info = await invoke<BaseInfo>("create_base", { name: baseNameFromPath(path), path });
      setBases((prev) => [...prev, info]);
      setSelectedPath(info.path);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>Importer dans quelle base ?</h2>
        <p className="picker-path">{pgnPath}</p>
        {error && <p className="error">{error}</p>}
        <div className="modal-table-wrap">
          <BasesTable bases={bases} selectedPath={selectedPath} onSelect={(b) => setSelectedPath(b.path)} />
        </div>
        <div className="modal-actions">
          <button onClick={handleCreateBase}>Nouvelle base</button>
          <button onClick={onCancel}>Annuler</button>
          <button disabled={!selectedPath} onClick={() => selectedPath && onConfirm(selectedPath)}>
            Importer
          </button>
        </div>
      </div>
    </div>
  );
}
