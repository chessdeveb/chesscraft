import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { documentDir, join } from "@tauri-apps/api/path";
import { ask, open } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";
import { toMegabaseError, type MegabaseDownload } from "../useMegabaseDownload";
import type { MegabaseError, MegabaseManifest } from "../types";

function useFormatters() {
  const { t, i18n } = useTranslation();
  const lang = i18n.language;
  const gb = (bytes: number) =>
    t("megabase.gigabytes", {
      value: (bytes / 1e9).toLocaleString(lang, { minimumFractionDigits: 1, maximumFractionDigits: 1 }),
    });
  const count = (n: number) => n.toLocaleString(lang);
  const date = (version: string) => {
    const d = new Date(`${version}T12:00:00`);
    return Number.isNaN(d.getTime()) ? version : d.toLocaleDateString(lang, { day: "numeric", month: "long", year: "numeric" });
  };
  const errorText = (error: MegabaseError) => {
    const key = `megabase.error.${error.code}`;
    const text = t(key, {
      detail: error.detail ?? "",
      needed: error.needed != null ? gb(error.needed) : "",
      available: error.available != null ? gb(error.available) : "",
    });
    return text === key ? (error.detail ?? error.code) : text;
  };
  return { gb, count, date, errorText };
}

function eta(t: (key: string, options?: Record<string, unknown>) => string, remaining: number, bytesPerSec: number): string | null {
  if (bytesPerSec <= 0) return null;
  const minutes = Math.max(1, Math.round(remaining / bytesPerSec / 60));
  if (minutes < 60) return t("megabase.etaMinutes", { count: minutes });
  return t("megabase.etaHours", { hours: Math.floor(minutes / 60), minutes: minutes % 60 });
}

/** Premier ecran : description, taille, choix du dossier. */
function MegabaseIntro({ download }: { download: MegabaseDownload }) {
  const { t } = useTranslation();
  const { gb, count, date, errorText } = useFormatters();
  const [manifest, setManifest] = useState<MegabaseManifest | null>(null);
  const [loadError, setLoadError] = useState<MegabaseError | null>(null);
  const [startError, setStartError] = useState<MegabaseError | null>(null);
  const [dir, setDir] = useState("");
  const [starting, setStarting] = useState(false);

  const loadManifest = useCallback(() => {
    setLoadError(null);
    invoke<MegabaseManifest>("megabase_manifest")
      .then(setManifest)
      .catch((e) => setLoadError(toMegabaseError(e)));
  }, []);

  useEffect(() => {
    loadManifest();
    documentDir()
      .then((docs) => join(docs, "ChessCraft"))
      .then(setDir)
      .catch(() => {});
  }, [loadManifest]);

  const chooseFolder = useCallback(async () => {
    const picked = await open({ directory: true, multiple: false, defaultPath: dir || undefined });
    if (typeof picked === "string") {
      setDir(picked);
      setStartError(null);
    }
  }, [dir]);

  const handleStart = useCallback(async () => {
    if (!manifest || !dir) return;
    setStarting(true);
    setStartError(null);
    try {
      await download.start(dir, manifest);
    } catch (e) {
      setStartError(toMegabaseError(e));
    } finally {
      setStarting(false);
    }
  }, [download, manifest, dir]);

  return (
    <>
      <h2>{t("megabase.title")}</h2>
      {!manifest && !loadError && <p className="status-message">{t("megabase.loadingManifest")}</p>}
      {loadError && <p className="error">{errorText(loadError)}</p>}
      {manifest && (
        <>
          <p>{t("megabase.intro", { games: count(manifest.games), date: date(manifest.version) })}</p>
          <dl className="megabase-facts">
            <dt>{t("megabase.downloadSize")}</dt>
            <dd>{gb(manifest.compressedSize)}</dd>
            <dt>{t("megabase.diskSpace")}</dt>
            <dd>
              {t("megabase.diskSpaceValue", {
                during: gb(manifest.compressedSize + manifest.size),
                after: gb(manifest.size),
              })}
            </dd>
            <dt>{t("megabase.folder")}</dt>
            <dd className="megabase-folder">
              <span className="megabase-folder-path" title={dir}>
                {dir}
              </span>
              <button onClick={chooseFolder}>{t("megabase.chooseFolder")}</button>
            </dd>
          </dl>
          <p className="megabase-note">{t("megabase.optional")}</p>
          <p className="megabase-note">{t("megabase.license")}</p>
          {startError && <p className="error">{errorText(startError)}</p>}
        </>
      )}
      <div className="modal-actions">
        <button onClick={download.closeDialog}>{t("megabase.later")}</button>
        {loadError ? (
          <button onClick={loadManifest}>{t("megabase.retry")}</button>
        ) : (
          <button className="megabase-primary" onClick={handleStart} disabled={!manifest || !dir || starting}>
            {t("megabase.start")}
          </button>
        )}
      </div>
    </>
  );
}

export function MegabaseDialog({ download }: { download: MegabaseDownload }) {
  const { t } = useTranslation();
  const { gb, count, errorText } = useFormatters();
  const { phase, progress, pending, error, installed } = download;

  const confirmCancel = useCallback(async () => {
    const confirmed = await ask(t("megabase.cancelConfirm"), { title: t("megabase.title"), kind: "warning" });
    if (confirmed) await download.cancel();
  }, [download, t]);

  let body;
  if (phase === "idle") {
    body = <MegabaseIntro download={download} />;
  } else if (phase === "running") {
    const percent = progress && progress.total > 0 ? (progress.done / progress.total) * 100 : 0;
    const details = progress
      ? [
          t("megabase.progressOf", { done: gb(progress.done), total: gb(progress.total) }),
          progress.bytesPerSec > 0
            ? t("megabase.speed", {
                value: (progress.bytesPerSec / 1e6).toLocaleString(undefined, { maximumFractionDigits: 1 }),
              })
            : null,
          eta(t, progress.total - progress.done, progress.bytesPerSec),
        ]
          .filter(Boolean)
          .join(" · ")
      : t("megabase.connecting");
    body = (
      <>
        <h2>{t("megabase.runningTitle")}</h2>
        <p className="megabase-stage">{t(`megabase.stage.${progress?.stage ?? "download"}`)}</p>
        <progress className="megabase-progress" value={percent} max={100} />
        <p className="megabase-details">{details}</p>
        <p className="megabase-note">{t("megabase.runningHint")}</p>
        <div className="modal-actions">
          <button onClick={confirmCancel}>{t("megabase.cancel")}</button>
          <button onClick={download.pause}>{t("megabase.pause")}</button>
          <button className="megabase-primary" onClick={download.closeDialog}>
            {t("megabase.hide")}
          </button>
        </div>
      </>
    );
  } else if (phase === "paused" || phase === "error") {
    body = (
      <>
        <h2>{phase === "paused" ? t("megabase.pausedTitle") : t("megabase.errorTitle")}</h2>
        {error && phase === "error" && <p className="error">{errorText(error)}</p>}
        {pending && (
          <p>
            {t("megabase.pausedText", {
              done: gb(pending.downloaded),
              total: gb(pending.manifest.compressedSize),
              dir: pending.dir,
            })}
          </p>
        )}
        <div className="modal-actions">
          {pending && <button onClick={confirmCancel}>{t("megabase.cancel")}</button>}
          <button onClick={download.closeDialog}>{t("megabase.close")}</button>
          {pending && (
            <button className="megabase-primary" onClick={download.resume}>
              {phase === "paused" ? t("megabase.resume") : t("megabase.retry")}
            </button>
          )}
        </div>
      </>
    );
  } else {
    body = (
      <>
        <h2>{t("megabase.doneTitle")}</h2>
        {installed && (
          <p>
            {t("megabase.doneText", { games: count(installed.games), name: installed.name })}
            {installed.isReference && <> {t("megabase.doneReference")}</>}
          </p>
        )}
        <div className="modal-actions">
          <button className="megabase-primary" onClick={download.closeDialog}>
            {t("common.ok")}
          </button>
        </div>
      </>
    );
  }

  return (
    <div className="modal-overlay">
      <div className="modal megabase-dialog" role="dialog" aria-modal="true">
        {body}
      </div>
    </div>
  );
}
