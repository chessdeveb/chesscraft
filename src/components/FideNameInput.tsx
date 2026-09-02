import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { FidePlayer, FidePlayersPage } from "../types";

interface FideNameInputProps {
  id: string;
  value: string;
  onChange: (value: string) => void;
  onSelectPlayer?: (player: FidePlayer) => void;
}

// Autocompletion/verification par rapport a la liste FIDE locale (voir
// fide.rs) : un <input> classique associe a un <datalist>, pas de
// composant de suggestions maison -- le navigateur (WebView2 ici, moteur
// fixe et connu) gere deja le filtrage clavier/affichage, et le <datalist>
// reste utilisable meme si la liste FIDE n'a jamais ete telechargee
// (options simplement vides).
export function FideNameInput({ id, value, onChange, onSelectPlayer }: FideNameInputProps) {
  const [options, setOptions] = useState<FidePlayer[]>([]);
  const requestIdRef = useRef(0);
  const listId = `${id}-fide-list`;

  useEffect(() => {
    const query = value.trim();
    if (query.length < 2) {
      setOptions([]);
      return;
    }
    const requestId = ++requestIdRef.current;
    // debounce : eviter un invoke a chaque frappe pendant la saisie
    const timeout = setTimeout(() => {
      invoke<FidePlayersPage>("list_fide_players", {
        search: query,
        sortBy: "std",
        sortDir: "desc",
        offset: 0,
        limit: 8,
      })
        .then((res) => {
          if (requestIdRef.current !== requestId) return;
          setOptions(res.players);
        })
        .catch(() => {
          if (requestIdRef.current !== requestId) return;
          setOptions([]);
        });
    }, 200);
    return () => clearTimeout(timeout);
  }, [value]);

  function handleChange(next: string) {
    onChange(next);
    // le nom choisi dans le datalist correspond exactement a une des
    // options proposees : on en profite pour remonter le joueur complet
    // (Elo...) au parent plutot que de forcer une seconde recherche.
    const match = options.find((p) => p.name === next);
    if (match) onSelectPlayer?.(match);
  }

  return (
    <>
      <input list={listId} value={value} onChange={(e) => handleChange(e.target.value)} />
      <datalist id={listId}>
        {options.map((p) => (
          <option key={p.fideId} value={p.name}>
            {[p.fed, p.title, p.stdRating ? `${p.stdRating}` : null].filter(Boolean).join(" · ")}
          </option>
        ))}
      </datalist>
    </>
  );
}
