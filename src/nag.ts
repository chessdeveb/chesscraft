export interface NagChoice {
  code: number;
  symbol: string;
  // cle de traduction (voir src/locales/*.json), pas le texte final : ces
  // tableaux sont des constantes de module evaluees une seule fois, avant
  // que i18next ait forcement choisi une langue -- resoudre la cle avec
  // t(n.labelKey) au moment du rendu plutot que de figer le texte ici.
  labelKey: string;
}

export interface NagCluster {
  id: string;
  // Contenu du bouton declencheur (glyphe unicode ou courte abreviation --
  // beaucoup de NAG positionnels n'ont pas de symbole traditionnel etabli).
  symbol: string;
  // null pour les clusters sans camp (qualite du coup, editorial...) ou
  // pour les clusters autoSide (voir plus bas -- le symbole est identique
  // des deux cotes, pas besoin de point pour les distinguer).
  side: "white" | "black" | null;
  titleKey: string;
  // Pour un cluster autoSide : exactement [code Blancs, code Noirs].
  // Sinon : la liste proposee dans la liste deroulante, dans l'ordre
  // d'affichage.
  entries: NagChoice[];
  // Le symbole Blancs et le symbole Noirs sont visuellement identiques
  // (espace, developpement, attaque...) : plutot que deux entrees dans une
  // liste, un seul bouton a acces direct qui choisit automatiquement le
  // code Blancs ou Noirs selon la couleur du coup courant -- voir
  // NagClusterButton.tsx. entries doit alors contenir exactement 2 codes,
  // dans l'ordre [Blancs, Noirs].
  autoSide?: boolean;
  // Sous-groupes d'exclusivite internes au cluster (un seul NAG actif par
  // sous-groupe a la fois, voir nagExclusivityGroup ci-dessous). Absent =
  // tout le cluster ne forme qu'un seul groupe (cas normal : le cluster
  // represente une seule echelle graduee, ex. espace leger/moyen/decisif).
  // Necessaire seulement quand un cluster regroupe pour l'affichage
  // plusieurs echelles independantes sous un meme bouton (ex. "Roi" melange
  // securite et placement : etre bien protege ne doit pas effacer etre mal
  // place, ce sont deux jugements distincts). Un tableau vide (pas de
  // sous-groupes du tout, chaque code se bascule independamment) est
  // utilise pour les listes qui ne sont pas une echelle graduee (editorial,
  // zones du plateau...).
  exclusivityGroups?: number[][];
}

// Catalogue plat de tous les NAG geres par l'appli : les $1-$139 du
// standard PGN (section 10 de la specification) plus une selection de
// glyphes non-standards largement repandus au-dela de $139 -- ChessPad/
// ChessBase notamment, voir
// https://en.wikipedia.org/wiki/Numeric_Annotation_Glyphs et
// https://github.com/mliebelt/pgn-spec-commented/blob/main/annotated/nag-research.md
// pour les references. $0 (annotation nulle) est volontairement absent : ce
// n'est pas une annotation reelle. Les plages non definies (146-219,
// 222-237) sont omises. Regroupe en clusters ci-dessous pour la barre
// d'annotation (voir NagClusterButton.tsx) : chaque bouton ouvre une petite
// liste verticale plutot que d'afficher tout a plat (165 boutons ne
// tiendraient pas dans une barre d'outils).
const NAG_ENTRIES: NagChoice[] = [
  { code: 1, symbol: "!", labelKey: "nagAll.1" },
  { code: 2, symbol: "?", labelKey: "nagAll.2" },
  { code: 3, symbol: "!!", labelKey: "nagAll.3" },
  { code: 4, symbol: "??", labelKey: "nagAll.4" },
  { code: 5, symbol: "!?", labelKey: "nagAll.5" },
  { code: 6, symbol: "?!", labelKey: "nagAll.6" },
  { code: 7, symbol: "□", labelKey: "nagAll.7" },
  { code: 8, symbol: "", labelKey: "nagAll.8" },
  { code: 9, symbol: "", labelKey: "nagAll.9" },
  { code: 10, symbol: "=", labelKey: "nagAll.10" },
  { code: 11, symbol: "", labelKey: "nagAll.11" },
  { code: 12, symbol: "", labelKey: "nagAll.12" },
  { code: 13, symbol: "∞", labelKey: "nagAll.13" },
  { code: 14, symbol: "⩲", labelKey: "nagAll.14" },
  { code: 15, symbol: "⩱", labelKey: "nagAll.15" },
  { code: 16, symbol: "±", labelKey: "nagAll.16" },
  { code: 17, symbol: "∓", labelKey: "nagAll.17" },
  { code: 18, symbol: "+−", labelKey: "nagAll.18" },
  { code: 19, symbol: "−+", labelKey: "nagAll.19" },
  { code: 20, symbol: "", labelKey: "nagAll.20" },
  { code: 21, symbol: "", labelKey: "nagAll.21" },
  { code: 22, symbol: "⨀", labelKey: "nagAll.22" },
  { code: 23, symbol: "⨀", labelKey: "nagAll.23" },
  { code: 24, symbol: "", labelKey: "nagAll.24" },
  { code: 25, symbol: "", labelKey: "nagAll.25" },
  { code: 26, symbol: "○", labelKey: "nagAll.26" },
  { code: 27, symbol: "○", labelKey: "nagAll.27" },
  { code: 28, symbol: "", labelKey: "nagAll.28" },
  { code: 29, symbol: "", labelKey: "nagAll.29" },
  { code: 30, symbol: "", labelKey: "nagAll.30" },
  { code: 31, symbol: "", labelKey: "nagAll.31" },
  { code: 32, symbol: "⟳", labelKey: "nagAll.32" },
  { code: 33, symbol: "⟳", labelKey: "nagAll.33" },
  { code: 34, symbol: "", labelKey: "nagAll.34" },
  { code: 35, symbol: "", labelKey: "nagAll.35" },
  { code: 36, symbol: "↑", labelKey: "nagAll.36" },
  { code: 37, symbol: "↑", labelKey: "nagAll.37" },
  { code: 38, symbol: "", labelKey: "nagAll.38" },
  { code: 39, symbol: "", labelKey: "nagAll.39" },
  { code: 40, symbol: "→", labelKey: "nagAll.40" },
  { code: 41, symbol: "→", labelKey: "nagAll.41" },
  { code: 42, symbol: "", labelKey: "nagAll.42" },
  { code: 43, symbol: "", labelKey: "nagAll.43" },
  { code: 44, symbol: "=∞", labelKey: "nagAll.44" },
  { code: 45, symbol: "=∞", labelKey: "nagAll.45" },
  { code: 46, symbol: "", labelKey: "nagAll.46" },
  { code: 47, symbol: "", labelKey: "nagAll.47" },
  { code: 48, symbol: "", labelKey: "nagAll.48" },
  { code: 49, symbol: "", labelKey: "nagAll.49" },
  { code: 50, symbol: "", labelKey: "nagAll.50" },
  { code: 51, symbol: "", labelKey: "nagAll.51" },
  { code: 52, symbol: "", labelKey: "nagAll.52" },
  { code: 53, symbol: "", labelKey: "nagAll.53" },
  { code: 54, symbol: "", labelKey: "nagAll.54" },
  { code: 55, symbol: "", labelKey: "nagAll.55" },
  { code: 56, symbol: "", labelKey: "nagAll.56" },
  { code: 57, symbol: "", labelKey: "nagAll.57" },
  { code: 58, symbol: "", labelKey: "nagAll.58" },
  { code: 59, symbol: "", labelKey: "nagAll.59" },
  { code: 60, symbol: "", labelKey: "nagAll.60" },
  { code: 61, symbol: "", labelKey: "nagAll.61" },
  { code: 62, symbol: "", labelKey: "nagAll.62" },
  { code: 63, symbol: "", labelKey: "nagAll.63" },
  { code: 64, symbol: "", labelKey: "nagAll.64" },
  { code: 65, symbol: "", labelKey: "nagAll.65" },
  { code: 66, symbol: "", labelKey: "nagAll.66" },
  { code: 67, symbol: "", labelKey: "nagAll.67" },
  { code: 68, symbol: "", labelKey: "nagAll.68" },
  { code: 69, symbol: "", labelKey: "nagAll.69" },
  { code: 70, symbol: "", labelKey: "nagAll.70" },
  { code: 71, symbol: "", labelKey: "nagAll.71" },
  { code: 72, symbol: "", labelKey: "nagAll.72" },
  { code: 73, symbol: "", labelKey: "nagAll.73" },
  { code: 74, symbol: "", labelKey: "nagAll.74" },
  { code: 75, symbol: "", labelKey: "nagAll.75" },
  { code: 76, symbol: "", labelKey: "nagAll.76" },
  { code: 77, symbol: "", labelKey: "nagAll.77" },
  { code: 78, symbol: "", labelKey: "nagAll.78" },
  { code: 79, symbol: "", labelKey: "nagAll.79" },
  { code: 80, symbol: "", labelKey: "nagAll.80" },
  { code: 81, symbol: "", labelKey: "nagAll.81" },
  { code: 82, symbol: "", labelKey: "nagAll.82" },
  { code: 83, symbol: "", labelKey: "nagAll.83" },
  { code: 84, symbol: "", labelKey: "nagAll.84" },
  { code: 85, symbol: "", labelKey: "nagAll.85" },
  { code: 86, symbol: "", labelKey: "nagAll.86" },
  { code: 87, symbol: "", labelKey: "nagAll.87" },
  { code: 88, symbol: "", labelKey: "nagAll.88" },
  { code: 89, symbol: "", labelKey: "nagAll.89" },
  { code: 90, symbol: "", labelKey: "nagAll.90" },
  { code: 91, symbol: "", labelKey: "nagAll.91" },
  { code: 92, symbol: "", labelKey: "nagAll.92" },
  { code: 93, symbol: "", labelKey: "nagAll.93" },
  { code: 94, symbol: "", labelKey: "nagAll.94" },
  { code: 95, symbol: "", labelKey: "nagAll.95" },
  { code: 96, symbol: "", labelKey: "nagAll.96" },
  { code: 97, symbol: "", labelKey: "nagAll.97" },
  { code: 98, symbol: "", labelKey: "nagAll.98" },
  { code: 99, symbol: "", labelKey: "nagAll.99" },
  { code: 100, symbol: "", labelKey: "nagAll.100" },
  { code: 101, symbol: "", labelKey: "nagAll.101" },
  { code: 102, symbol: "", labelKey: "nagAll.102" },
  { code: 103, symbol: "", labelKey: "nagAll.103" },
  { code: 104, symbol: "", labelKey: "nagAll.104" },
  { code: 105, symbol: "", labelKey: "nagAll.105" },
  { code: 106, symbol: "", labelKey: "nagAll.106" },
  { code: 107, symbol: "", labelKey: "nagAll.107" },
  { code: 108, symbol: "", labelKey: "nagAll.108" },
  { code: 109, symbol: "", labelKey: "nagAll.109" },
  { code: 110, symbol: "", labelKey: "nagAll.110" },
  { code: 111, symbol: "", labelKey: "nagAll.111" },
  { code: 112, symbol: "", labelKey: "nagAll.112" },
  { code: 113, symbol: "", labelKey: "nagAll.113" },
  { code: 114, symbol: "", labelKey: "nagAll.114" },
  { code: 115, symbol: "", labelKey: "nagAll.115" },
  { code: 116, symbol: "", labelKey: "nagAll.116" },
  { code: 117, symbol: "", labelKey: "nagAll.117" },
  { code: 118, symbol: "", labelKey: "nagAll.118" },
  { code: 119, symbol: "", labelKey: "nagAll.119" },
  { code: 120, symbol: "", labelKey: "nagAll.120" },
  { code: 121, symbol: "", labelKey: "nagAll.121" },
  { code: 122, symbol: "", labelKey: "nagAll.122" },
  { code: 123, symbol: "", labelKey: "nagAll.123" },
  { code: 124, symbol: "", labelKey: "nagAll.124" },
  { code: 125, symbol: "", labelKey: "nagAll.125" },
  { code: 126, symbol: "", labelKey: "nagAll.126" },
  { code: 127, symbol: "", labelKey: "nagAll.127" },
  { code: 128, symbol: "", labelKey: "nagAll.128" },
  { code: 129, symbol: "", labelKey: "nagAll.129" },
  { code: 130, symbol: "", labelKey: "nagAll.130" },
  { code: 131, symbol: "", labelKey: "nagAll.131" },
  { code: 132, symbol: "⇆", labelKey: "nagAll.132" },
  { code: 133, symbol: "⇆", labelKey: "nagAll.133" },
  { code: 134, symbol: "", labelKey: "nagAll.134" },
  { code: 135, symbol: "", labelKey: "nagAll.135" },
  { code: 136, symbol: "", labelKey: "nagAll.136" },
  { code: 137, symbol: "", labelKey: "nagAll.137" },
  { code: 138, symbol: "⨁", labelKey: "nagAll.138" },
  { code: 139, symbol: "⨁", labelKey: "nagAll.139" },
  { code: 140, symbol: "∆", labelKey: "nagAll.140" },
  { code: 141, symbol: "∇", labelKey: "nagAll.141" },
  { code: 142, symbol: "⌓", labelKey: "nagAll.142" },
  { code: 143, symbol: "≤", labelKey: "nagAll.143" },
  { code: 144, symbol: "==", labelKey: "nagAll.144" },
  { code: 145, symbol: "", labelKey: "nagAll.145" },
  { code: 220, symbol: "⬒", labelKey: "nagAll.220" },
  { code: 221, symbol: "⬓", labelKey: "nagAll.221" },
  { code: 238, symbol: "○", labelKey: "nagAll.238" },
  { code: 239, symbol: "⇔", labelKey: "nagAll.239" },
  { code: 240, symbol: "⇗", labelKey: "nagAll.240" },
  { code: 241, symbol: "⊞", labelKey: "nagAll.241" },
  { code: 242, symbol: "⟫", labelKey: "nagAll.242" },
  { code: 243, symbol: "⟪", labelKey: "nagAll.243" },
  { code: 244, symbol: "✕", labelKey: "nagAll.244" },
  { code: 245, symbol: "⊥", labelKey: "nagAll.245" },
  { code: 246, symbol: "", labelKey: "nagAll.246" },
  { code: 247, symbol: "", labelKey: "nagAll.247" },
  { code: 248, symbol: "", labelKey: "nagAll.248" },
  { code: 249, symbol: "", labelKey: "nagAll.249" },
  { code: 250, symbol: "", labelKey: "nagAll.250" },
  { code: 251, symbol: "", labelKey: "nagAll.251" },
  { code: 252, symbol: "", labelKey: "nagAll.252" },
  { code: 253, symbol: "", labelKey: "nagAll.253" },
  { code: 254, symbol: "", labelKey: "nagAll.254" },
  { code: 255, symbol: "", labelKey: "nagAll.255" },
];

const nagByCode = new Map(NAG_ENTRIES.map((n) => [n.code, n]));

function pick(...codes: number[]): NagChoice[] {
  return codes.map((code) => {
    const entry = nagByCode.get(code);
    if (!entry) throw new Error(`NAG $${code} inconnu`);
    return entry;
  });
}

// Chaque code n'est independant que dans sa propre petite liste (pas de
// recherche/liste geante unique) -- voir NagClusterButton.tsx. N'inclut que
// les codes ayant un veritable symbole (voir NAG_ENTRIES ci-dessus) : la
// plupart des NAG positionnels ($48-$129, $246-$255...) n'ont jamais eu de
// glyphe traditionnel etabli et n'ont donc pas leur place dans un
// selecteur visuel -- ils restent geres cote donnees/export PGN (n'importe
// quel code $0-$255 reste lisible/ecrit normalement, voir nagSymbol et
// pgn::write_line), simplement plus proposes ici. Les clusters qui
// n'auraient plus aucune entree (tous leurs codes sans symbole) sont
// retires entierement plutot que de garder un bouton vide.
export const NAG_CLUSTERS: NagCluster[] = [
  { id: "moveQuality", symbol: "!", side: null, titleKey: "nagCluster.moveQuality", entries: pick(1, 2, 3, 4, 5, 6, 7) },
  { id: "advantageWhite", symbol: "±", side: null, titleKey: "nagCluster.advantageWhite", entries: pick(14, 16, 18) },
  { id: "advantageBlack", symbol: "∓", side: null, titleKey: "nagCluster.advantageBlack", entries: pick(15, 17, 19) },
  { id: "neutral", symbol: "=", side: null, titleKey: "nagCluster.neutral", entries: pick(10, 13, 22, 23) },
  // Blancs/Noirs fusionnes en acces direct quand les deux partagent le meme
  // symbole (rien ne les distingue visuellement de toute facon) : un seul
  // bouton, le code Blancs ou Noirs est choisi automatiquement selon la
  // couleur du coup courant (voir autoSide, NagClusterButton.tsx). Restent
  // separes en deux boutons seulement quand le symbole differe par camp
  // (avantage : ± vs ∓).
  { id: "space", symbol: "○", side: null, titleKey: "nagCluster.space", entries: pick(26, 27), autoSide: true },
  { id: "development", symbol: "⟳", side: null, titleKey: "nagCluster.development", entries: pick(32, 33), autoSide: true },
  { id: "initiative", symbol: "↑", side: null, titleKey: "nagCluster.initiative", entries: pick(36, 37), autoSide: true },
  { id: "attack", symbol: "→", side: null, titleKey: "nagCluster.attack", entries: pick(40, 41), autoSide: true },
  { id: "compensation", symbol: "=∞", side: null, titleKey: "nagCluster.compensation", entries: pick(44, 45), autoSide: true },
  { id: "counterplay", symbol: "⇆", side: null, titleKey: "nagCluster.counterplay", entries: pick(132, 133), autoSide: true },
  { id: "timePressure", symbol: "⨁", side: null, titleKey: "nagCluster.timePressure", entries: pick(138, 139), autoSide: true },
  {
    id: "editorial",
    symbol: "✎",
    side: null,
    titleKey: "nagCluster.editorial",
    entries: pick(140, 141, 142, 143, 144, 220, 221),
    // Tableau vide = independants par defaut (voir nagExclusivityGroup) ;
    // 220/221 sont l'exception : deux orientations du meme diagramme,
    // insere un seul a la fois (voir renderLine dans Notation.tsx).
    exclusivityGroups: [[220, 221]],
  },
  {
    id: "positionalZones",
    symbol: "⇔",
    side: null,
    titleKey: "nagCluster.positionalZones",
    entries: pick(238, 239, 240, 241, 242, 243, 244, 245),
    exclusivityGroups: [],
  },
];

/** Codes "editoriaux" (avec l'idee de..., mieux vaut..., diagramme...) --
    contrairement aux autres NAG (qualite du coup, evaluation...), ils
    introduisent ce qui suit plutot que de qualifier le coup lui-meme : la
    notation les affiche donc AVANT le coup, pas apres (voir Notation.tsx).
    N'affecte que l'affichage cote appli -- l'export PGN garde l'ordre
    standard (NAG juste apres le coup, voir pgn::write_line), pour rester
    lisible par n'importe quel autre logiciel d'echecs. */
export const PREFIX_NAG_CODES = new Set(NAG_CLUSTERS.find((c) => c.id === "editorial")?.entries.map((e) => e.code) ?? []);

/** Groupes d'exclusivite qui debordent d'un seul cluster (voir
    nagExclusivityGroup ci-dessous) : avantage Blancs et avantage Noirs
    restent deux boutons separes (symboles differents, ± vs ∓), mais
    choisir "avantage decisif des Blancs" doit quand meme effacer un
    "avantage des Noirs" deja actif sur le meme coup -- ce sont deux
    jugements contradictoires sur la meme position, pas deux annotations
    independantes. Meme raisonnement pour les trois jugements comparatifs
    du bouton editorial (mieux vaut / pire etait / equivalent a). Verifie
    avant la logique par cluster ci-dessous. */
const CROSS_CLUSTER_EXCLUSIVITY_GROUPS: number[][] = [
  [14, 16, 18, 15, 17, 19],
  [142, 143, 144],
];

/** Groupe d'exclusivite du code donne (voir NagCluster.exclusivityGroups) :
    activer un NAG de ce groupe desactive les autres codes du meme groupe.
    Toujours au moins le code lui-meme (aucune exclusivite si le code
    n'appartient a aucun cluster connu, ne devrait pas arriver). */
export function nagExclusivityGroup(code: number): Set<number> {
  const cross = CROSS_CLUSTER_EXCLUSIVITY_GROUPS.find((g) => g.includes(code));
  if (cross) return new Set(cross);
  for (const cluster of NAG_CLUSTERS) {
    if (!cluster.entries.some((e) => e.code === code)) continue;
    if (cluster.exclusivityGroups) {
      const sub = cluster.exclusivityGroups.find((g) => g.includes(code));
      return new Set(sub ?? [code]);
    }
    return new Set(cluster.entries.map((e) => e.code));
  }
  return new Set([code]);
}

const NAG_SYMBOLS: Record<number, string> = Object.fromEntries(NAG_ENTRIES.map((n) => [n.code, n.symbol]));

// "" quand le code n'a pas de glyphe traditionnel etabli (la plupart des
// NAG positionnels au-dela des tout premiers) -- pas de repli "$N" : un
// coup sans glyphe connu n'affiche simplement rien de plus apres son SAN.
export function nagSymbol(code: number): string {
  return NAG_SYMBOLS[code] ?? "";
}
