import type { EngineInfo } from "./types";

/** Part des Blancs (0 = gain noir certain, 1 = gain blanc certain) pour la
    barre d'evaluation. Utilise les probabilites gain/nulle/perte du moteur
    quand il les fournit (UCI_ShowWDL, voir engine.rs) ; sinon la conversion
    centipions -> chances de gain de Lichess, deja utilisee par le rapport de
    partie cote Rust. */
export function whiteShare(info: EngineInfo): number {
  if (info.scoreMate !== null) {
    if (info.scoreMate > 0) return 1;
    if (info.scoreMate < 0) return 0;
  }
  if (info.wdl) {
    const [win, draw, loss] = info.wdl;
    const total = win + draw + loss;
    if (total > 0) return (win + draw / 2) / total;
  }
  if (info.scoreCp !== null) return 1 / (1 + Math.exp(-0.00368208 * info.scoreCp));
  return 0.5;
}

/** Score court pour l'etiquette de la barre : "+0.4", "-1.2", "#5". */
export function shortScore(info: EngineInfo): string {
  if (info.scoreMate !== null) return `#${Math.abs(info.scoreMate)}`;
  if (info.scoreCp === null) return "";
  const value = Math.abs(info.scoreCp / 100).toFixed(1);
  return info.scoreCp > 0 ? `+${value}` : info.scoreCp < 0 ? `-${value}` : "0.0";
}
