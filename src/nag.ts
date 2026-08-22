export interface NagChoice {
  code: number;
  symbol: string;
  label: string;
}

// qualite du coup joue (independant de l'evaluation de la position)
export const MOVE_NAG_CHOICES: NagChoice[] = [
  { code: 3, symbol: "!!", label: "coup brillant" },
  { code: 1, symbol: "!", label: "bon coup" },
  { code: 5, symbol: "!?", label: "coup interessant" },
  { code: 6, symbol: "?!", label: "coup douteux" },
  { code: 2, symbol: "?", label: "erreur" },
  { code: 4, symbol: "??", label: "gaffe" },
];

// evaluation de la position resultante (independante de la qualite du coup)
export const EVAL_NAG_CHOICES: NagChoice[] = [
  { code: 18, symbol: "+−", label: "avantage decisif des Blancs" },
  { code: 16, symbol: "±", label: "avantage des Blancs" },
  { code: 14, symbol: "⩲", label: "leger avantage des Blancs" },
  { code: 10, symbol: "=", label: "position egale" },
  { code: 13, symbol: "∞", label: "position peu claire" },
  { code: 15, symbol: "⩱", label: "leger avantage des Noirs" },
  { code: 17, symbol: "∓", label: "avantage des Noirs" },
  { code: 19, symbol: "−+", label: "avantage decisif des Noirs" },
];

export const MOVE_NAG_CODES = new Set(MOVE_NAG_CHOICES.map((n) => n.code));
export const EVAL_NAG_CODES = new Set(EVAL_NAG_CHOICES.map((n) => n.code));

const NAG_SYMBOLS: Record<number, string> = Object.fromEntries(
  [...MOVE_NAG_CHOICES, ...EVAL_NAG_CHOICES].map((n) => [n.code, n.symbol])
);

export function nagSymbol(code: number): string {
  return NAG_SYMBOLS[code] ?? `$${code}`;
}
