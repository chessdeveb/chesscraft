export const NAG_CHOICES: { code: number; symbol: string; label: string }[] = [
  { code: 3, symbol: "!!", label: "coup brillant" },
  { code: 1, symbol: "!", label: "bon coup" },
  { code: 5, symbol: "!?", label: "coup interessant" },
  { code: 6, symbol: "?!", label: "coup douteux" },
  { code: 2, symbol: "?", label: "erreur" },
  { code: 4, symbol: "??", label: "gaffe" },
];

const NAG_SYMBOLS: Record<number, string> = Object.fromEntries(
  NAG_CHOICES.map((n) => [n.code, n.symbol])
);

export function nagSymbol(code: number): string {
  return NAG_SYMBOLS[code] ?? `$${code}`;
}
