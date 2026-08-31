export interface NagChoice {
  code: number;
  symbol: string;
  // cle de traduction (voir src/locales/*.json), pas le texte final : ces
  // tableaux sont des constantes de module evaluees une seule fois, avant
  // que i18next ait forcement choisi une langue -- resoudre la cle avec
  // t(n.labelKey) au moment du rendu plutot que de figer le texte ici.
  labelKey: string;
}

// qualite du coup joue (independant de l'evaluation de la position)
export const MOVE_NAG_CHOICES: NagChoice[] = [
  { code: 3, symbol: "!!", labelKey: "nag.brilliant" },
  { code: 1, symbol: "!", labelKey: "nag.good" },
  { code: 5, symbol: "!?", labelKey: "nag.interesting" },
  { code: 6, symbol: "?!", labelKey: "nag.dubious" },
  { code: 2, symbol: "?", labelKey: "nag.mistake" },
  { code: 4, symbol: "??", labelKey: "nag.blunder" },
];

// evaluation de la position resultante (independante de la qualite du coup)
export const EVAL_NAG_CHOICES: NagChoice[] = [
  { code: 18, symbol: "+−", labelKey: "nag.whiteDecisive" },
  { code: 16, symbol: "±", labelKey: "nag.whiteAdvantage" },
  { code: 14, symbol: "⩲", labelKey: "nag.whiteSlight" },
  { code: 10, symbol: "=", labelKey: "nag.equal" },
  { code: 13, symbol: "∞", labelKey: "nag.unclear" },
  { code: 15, symbol: "⩱", labelKey: "nag.blackSlight" },
  { code: 17, symbol: "∓", labelKey: "nag.blackAdvantage" },
  { code: 19, symbol: "−+", labelKey: "nag.blackDecisive" },
];

export const MOVE_NAG_CODES = new Set(MOVE_NAG_CHOICES.map((n) => n.code));
export const EVAL_NAG_CODES = new Set(EVAL_NAG_CHOICES.map((n) => n.code));

const NAG_SYMBOLS: Record<number, string> = Object.fromEntries(
  [...MOVE_NAG_CHOICES, ...EVAL_NAG_CHOICES].map((n) => [n.code, n.symbol])
);

export function nagSymbol(code: number): string {
  return NAG_SYMBOLS[code] ?? `$${code}`;
}
