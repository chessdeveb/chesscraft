export type TimeControlCategory = "blitz" | "rapid" | "classical";

// Grammaire du tag PGN TimeControl (norme d'export PGN) : une etape s'ecrit
// "[coups/]secondes[+increment]" (ex. "300+3", "40/5400+30") ; plusieurs
// etapes se separent par ":" (ex. "40/5400+30:1800+30" -- 90 min pour 40
// coups avec +30s/coup, puis 30 min de plus toujours +30s/coup, la
// convention FIDE classique du "rajout au 40e coup"). Voir aussi
// AccountImportDialog/chess_com.rs/lichess.rs pour le meme concept de
// cadence cote import.
const STAGE_RE = /^(?:(\d+)\/)?(\d+)(?:\+(\d+))?$/;

interface ParsedStage {
  seconds: number;
  increment: number;
}

function parseStage(stage: string): ParsedStage | null {
  const m = STAGE_RE.exec(stage.trim());
  if (!m) return null;
  const seconds = Number.parseInt(m[2], 10);
  const increment = m[3] ? Number.parseInt(m[3], 10) : 0;
  if (!Number.isFinite(seconds) || seconds < 0) return null;
  return { seconds, increment };
}

/** Duree totale estimee (en minutes, par joueur) d'une cadence PGN --
    somme du temps de base de chaque etape, plus un increment estime sur 40
    coups (methode FIDE) en prenant l'increment de la premiere etape qui en
    precise un. null si le texte ne suit pas la norme TimeControl (y compris
    "?"/"-"/vide, qui signifient respectivement inconnu/sans cadence). */
export function estimatedMinutes(timeControl: string): number | null {
  const trimmed = timeControl.trim();
  if (trimmed === "" || trimmed === "?" || trimmed === "-") return null;

  const stages = trimmed.split(":").map(parseStage);
  if (stages.some((s) => s === null)) return null;
  const parsed = stages as ParsedStage[];

  const totalSeconds = parsed.reduce((sum, s) => sum + s.seconds, 0);
  const increment = parsed.find((s) => s.increment > 0)?.increment ?? 0;
  return totalSeconds / 60 + (40 * increment) / 60;
}

/** Classement Blitz/Rapide/Classique d'une cadence PGN, selon les seuils
    FIDE (coherents avec les colonnes std/rapid/blitz deja utilisees pour les
    joueurs FIDE dans l'appli, voir fide.rs) : Blitz < 10 min, Rapide de 10 a
    60 min, Classique >= 60 min. null si la cadence est vide/inconnue/mal
    formee -- aucun des trois boutons ne se coche alors automatiquement. */
export function classifyTimeControl(timeControl: string): TimeControlCategory | null {
  const minutes = estimatedMinutes(timeControl);
  if (minutes === null) return null;
  if (minutes < 10) return "blitz";
  if (minutes < 60) return "rapid";
  return "classical";
}
