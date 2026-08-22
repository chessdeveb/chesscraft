import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { BasesTable } from "./components/BasesTable";
import type { BaseInfo } from "./types";
import "./App.css";

export function BasesWindow() {
  const [bases, setBases] = useState<BaseInfo[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<BaseInfo[]>("list_bases")
      .then(setBases)
      .catch((e) => setError(String(e)));
  }, []);

  async function handleSelect(base: BaseInfo) {
    await emit("select-base", base.path);
    await getCurrentWindow().close();
  }

  return (
    <main className="picker-window">
      <h2>Emplacement des bases</h2>
      {error && <p className="error">{error}</p>}
      <div className="modal-table-wrap">
        <BasesTable bases={bases} onSelect={handleSelect} />
      </div>
    </main>
  );
}
