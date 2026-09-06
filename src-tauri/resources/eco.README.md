Source: https://github.com/lichess-org/chess-openings (fichiers a.tsv..e.tsv, fusionnes en un seul)
Licence : CC0-1.0 (domaine public, aucune attribution requise)
Recupere le 2026-09-06.

Format (TSV, un en-tete) : eco<TAB>name<TAB>pgn
- eco : code ECO (ex. "B00")
- name : nom de l'ouverture, anglais, "Famille: Variante, Sous-variante"
- pgn : coups avec numeros ("1. e4 e6 2. d4 d5"), SAN standard (garde le "+"/"#"
  d'echec/mat contrairement a mainline_sans -- a retirer/tolerer cote parsing)

Algorithme de classification suggere par la source elle-meme : chercher la
ligne correspondant a la position atteinte en reculant coup par coup jusqu'a
trouver une correspondance exacte -- chaque nom a une ligne la plus courte
unique, donc prendre le prefixe le plus long qui matche exactement une entree.
