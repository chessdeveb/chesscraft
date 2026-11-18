import { useEffect } from "react";
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import fr from "./locales/fr.json";
import en from "./locales/en.json";

export const LANGUAGE_KEY = "language";
export type SupportedLanguage = "fr" | "en";

// cles a plat ("menu.file.newGame") plutot qu'imbriquees : garde le meme
// fichier de langue simple a lire cote React ET cote Rust (embarque via
// include_str! pour le menu natif), sans avoir a parser un JSON imbrique
// des deux cotes pour retrouver une seule cle.
// Sans choix enregistre : la langue de Windows (navigator.language), comme le
// fait le cote Rust (voir i18n.rs::load_language, qui reste l'autorite et
// resynchronise ensuite via useSyncLanguage) -- evite un bref affichage en
// francais au demarrage pour un Windows en anglais.
function systemLanguage(): SupportedLanguage {
  return (navigator.language || "fr").toLowerCase().startsWith("fr") ? "fr" : "en";
}

export function loadStoredLanguage(): SupportedLanguage {
  try {
    const stored = localStorage.getItem(LANGUAGE_KEY);
    if (stored === "en" || stored === "fr") return stored;
  } catch {
    // ignore : retombe sur la langue du systeme
  }
  return systemLanguage();
}

i18n.use(initReactI18next).init({
  resources: {
    fr: { translation: fr },
    en: { translation: en },
  },
  lng: loadStoredLanguage(),
  fallbackLng: "fr",
  keySeparator: false,
  nsSeparator: false,
  interpolation: { escapeValue: false },
});

export function setLanguage(lang: SupportedLanguage) {
  i18n.changeLanguage(lang);
  try {
    localStorage.setItem(LANGUAGE_KEY, lang);
  } catch {
    // ignore
  }
}

// Le menu natif (cote Rust) est la seule source de changement de langue pour
// l'instant : synchronise l'etat React au demarrage (Rust fait autorite, sa
// langue persistee peut differer du localStorage de cette fenetre) et a
// chaque bascule via le menu Preferences > Langue.
export function useSyncLanguage() {
  useEffect(() => {
    invoke<string>("get_language")
      .then((lang) => setLanguage(lang === "en" ? "en" : "fr"))
      .catch(() => {});
    const unlistenPromise = listen<string>("language-changed", (event) => {
      setLanguage(event.payload === "en" ? "en" : "fr");
    });
    return () => {
      unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);
}

export default i18n;
