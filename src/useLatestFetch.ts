import { useEffect, useRef, type DependencyList } from "react";

// Relance `fetcher()` a chaque changement de `deps` (comme un useEffect
// normal), mais sans jamais avoir plus d'un appel en vol : si les deps
// changent pendant qu'un appel est deja en cours (ex. un coup joue sur
// l'echiquier pendant que l'explorateur interroge encore la base de
// reference pour le coup precedent), on ne relance rien tout de suite --
// des que l'appel en cours revient, si les deps ont change entre-temps, on
// relance immediatement pour l'etat le plus recent (les positions
// intermediaires jouees pendant l'attente ne declenchent jamais leur propre
// requete, seule la derniere compte). `onResult`/`onError` ne sont jamais
// appeles pour un resultat perime : uniquement pour le dernier appel d'une
// serie, une fois rattrape.
//
// Contrairement a un debounce classique (voir FideNameInput, `setTimeout`
// fixe), il n'y a pas de temporisation arbitraire : si le backend repond
// assez vite pour suivre chaque coup, ca se met a jour immediatement ; s'il
// ne peut pas suivre, ca rattrape des qu'il se libere, toujours sur la
// position la plus recente.
export function useLatestFetch<T>(
  enabled: boolean,
  deps: DependencyList,
  fetcher: () => Promise<T>,
  onResult: (result: T) => void,
  onError: (error: unknown) => void
) {
  const generationRef = useRef(0);
  const busyRef = useRef(false);
  const enabledRef = useRef(enabled);
  const fetcherRef = useRef(fetcher);
  const onResultRef = useRef(onResult);
  const onErrorRef = useRef(onError);
  enabledRef.current = enabled;
  fetcherRef.current = fetcher;
  onResultRef.current = onResult;
  onErrorRef.current = onError;

  useEffect(() => {
    generationRef.current += 1;
    if (!enabled || busyRef.current) return;

    (async () => {
      busyRef.current = true;
      try {
        let seenGeneration = -1;
        // enabledRef ici (pas le `enabled` capture par la fermeture) : si
        // desactive entre-temps (ex. panneau Explorateur ferme), la boucle
        // s'arrete des l'appel en cours revient plutot que d'en relancer un
        // de plus pour rien.
        while (seenGeneration !== generationRef.current && enabledRef.current) {
          seenGeneration = generationRef.current;
          try {
            const result = await fetcherRef.current();
            if (seenGeneration === generationRef.current) onResultRef.current(result);
          } catch (e) {
            if (seenGeneration === generationRef.current) onErrorRef.current(e);
          }
        }
      } finally {
        busyRef.current = false;
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}
