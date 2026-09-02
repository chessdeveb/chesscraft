/// <reference types="vite/client" />

// chessground cible ses sprites de pieces via un element <piece> (nom de
// balise, pas une classe -- voir chessground.cburnett.css : "piece.pawn.white").
// Utilise tel quel dans la palette de l'editeur de position (PositionEditorWindow)
// pour reutiliser ces memes sprites hors du plateau lui-meme. Depuis
// React 18.3/@types react-jsx, les elements intrinseques vivent sous
// React.JSX (augmentation du module "react"), plus le namespace global JSX
// nu -- le declarer la-bas serait ignore par le runtime automatique.
import "react";

declare module "react" {
  namespace JSX {
    interface IntrinsicElements {
      piece: React.DetailedHTMLProps<React.HTMLAttributes<HTMLElement>, HTMLElement>;
    }
  }
}
