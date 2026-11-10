import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { BaseInfo, MegabaseError, MegabaseManifest, MegabasePending, MegabaseProgress } from "./types";

export type MegabasePhase = "idle" | "running" | "paused" | "error" | "done";

export interface MegabaseDownload {
  /** true une fois l'etat initial lu (telechargement en cours ou en pause). */
  ready: boolean;
  phase: MegabasePhase;
  progress: MegabaseProgress | null;
  pending: MegabasePending | null;
  error: MegabaseError | null;
  installed: BaseInfo | null;
  dialogOpen: boolean;
  openDialog: () => void;
  closeDialog: () => void;
  start: (dir: string, manifest: MegabaseManifest) => Promise<void>;
  resume: () => Promise<void>;
  pause: () => void;
  cancel: () => Promise<void>;
}

interface MegabaseStatus {
  running: boolean;
  pending: MegabasePending | null;
}

/** Erreur renvoyee par invoke : MegabaseError structuree, ou texte brut. */
export function toMegabaseError(e: unknown): MegabaseError {
  if (e && typeof e === "object" && "code" in e) return e as MegabaseError;
  return { code: "io", detail: String(e), needed: null, available: null };
}

/**
 * Etat du telechargement de la megabase (voir megabase.rs), partage entre la
 * fenetre de telechargement et le badge du panneau Bases : le telechargement
 * continue quand la fenetre est masquee.
 */
export function useMegabaseDownload(onInstalled: (info: BaseInfo) => void): MegabaseDownload {
  const [ready, setReady] = useState(false);
  const [phase, setPhase] = useState<MegabasePhase>("idle");
  const [progress, setProgress] = useState<MegabaseProgress | null>(null);
  const [pending, setPending] = useState<MegabasePending | null>(null);
  const [error, setError] = useState<MegabaseError | null>(null);
  const [installed, setInstalled] = useState<BaseInfo | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const onInstalledRef = useRef(onInstalled);
  onInstalledRef.current = onInstalled;

  const refreshPending = useCallback(async () => {
    try {
      const status = await invoke<MegabaseStatus>("megabase_status");
      setPending(status.pending);
      return status;
    } catch {
      return null;
    }
  }, []);

  // Etat initial : un telechargement interrompu (appli fermee, coupure) est
  // propose a la reprise des l'ouverture de l'accueil.
  useEffect(() => {
    refreshPending().then((status) => {
      if (status?.running) {
        setPhase("running");
      } else if (status?.pending) {
        setPhase("paused");
        setDialogOpen(true);
      }
      setReady(true);
    });
  }, [refreshPending]);

  useEffect(() => {
    const unlistenPromises = [
      listen<MegabaseProgress>("megabase-progress", (event) => {
        setPhase("running");
        setProgress(event.payload);
      }),
      listen("megabase-paused", () => {
        setPhase("paused");
        refreshPending();
      }),
      listen("megabase-cancelled", () => {
        setPhase("idle");
        setProgress(null);
        setPending(null);
      }),
      listen<MegabaseError>("megabase-error", (event) => {
        setPhase("error");
        setError(event.payload);
        setDialogOpen(true);
        refreshPending();
      }),
      listen<BaseInfo>("megabase-done", (event) => {
        setPhase("done");
        setInstalled(event.payload);
        setPending(null);
        setProgress(null);
        setDialogOpen(true);
        onInstalledRef.current(event.payload);
      }),
    ];
    return () => {
      unlistenPromises.forEach((p) => p.then((unlisten) => unlisten()));
    };
  }, [refreshPending]);

  const start = useCallback(
    async (dir: string, manifest: MegabaseManifest) => {
      setError(null);
      setProgress(null);
      await invoke("megabase_start", { dir, manifest });
      setPhase("running");
      refreshPending();
    },
    [refreshPending]
  );

  const resume = useCallback(async () => {
    setError(null);
    try {
      await invoke("megabase_resume");
      setPhase("running");
    } catch (e) {
      setPhase("error");
      setError(toMegabaseError(e));
    }
  }, []);

  const pause = useCallback(() => {
    invoke("megabase_pause");
  }, []);

  const cancel = useCallback(async () => {
    await invoke("megabase_cancel");
    // en pause ou en erreur, l'annulation est immediate (pas d'evenement)
    if (phase !== "running") {
      setPhase("idle");
      setProgress(null);
      setPending(null);
      setError(null);
    }
  }, [phase]);

  const openDialog = useCallback(() => setDialogOpen(true), []);

  const closeDialog = useCallback(() => {
    setDialogOpen(false);
    setPhase((prev) => {
      if (prev === "done") return "idle";
      // erreur avant tout telechargement (ex. serveur injoignable) : rien a reprendre
      if (prev === "error" && !pending) return "idle";
      return prev;
    });
  }, [pending]);

  return {
    ready,
    phase,
    progress,
    pending,
    error,
    installed,
    dialogOpen,
    openDialog,
    closeDialog,
    start,
    resume,
    pause,
    cancel,
  };
}
