import { useEffect, useState, type Dispatch, type SetStateAction } from "react";

const AUTO_DISMISS_MS = 6000;

// Message d'erreur qui disparait tout seul apres un delai, plutot que de
// rester affiche indefiniment jusqu'a ce qu'une action finisse par reussir
// (ou que la fenetre se ferme) -- meme signature que useState pour rester un
// remplacement direct partout ou ce motif (const [error, setError] =
// useState<string | null>(null), suivi d'un <p className="error">{error}</p>)
// etait deja utilise.
export function useAutoDismissError(delayMs = AUTO_DISMISS_MS): [string | null, Dispatch<SetStateAction<string | null>>] {
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!error) return;
    const timeout = setTimeout(() => setError(null), delayMs);
    return () => clearTimeout(timeout);
  }, [error, delayMs]);

  return [error, setError];
}
