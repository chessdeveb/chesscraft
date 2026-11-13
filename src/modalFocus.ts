import { getCurrentWindow } from "@tauri-apps/api/window";

// Met au premier plan toute boite de dialogue qui s'ouvre, dans toutes les
// fenetres de l'appli : la fenetre elle-meme est ramenee devant, et le
// clavier est place dans la boite (premier champ de saisie, sinon la boite)
// au lieu de rester sur ce qui se trouve derriere. Fait une seule fois ici,
// par observation du DOM, plutot que dans chacune des boites (toutes
// construites sur .modal-overlay > .modal, voir App.css).

function focusModal(overlay: Element) {
  const modal = overlay.querySelector<HTMLElement>(".modal");
  if (!modal) return;
  getCurrentWindow()
    .setFocus()
    .catch(() => {});
  // un champ a deja pris le focus de lui-meme (autoFocus) : ne pas le lui reprendre
  if (modal.contains(document.activeElement)) return;
  const field = modal.querySelector<HTMLElement>("input:not([disabled]):not([type=hidden]), select:not([disabled]), textarea:not([disabled])");
  if (field) {
    field.focus();
    return;
  }
  if (!modal.hasAttribute("tabindex")) modal.setAttribute("tabindex", "-1");
  modal.focus();
}

export function installModalFocus() {
  const observer = new MutationObserver((mutations) => {
    for (const mutation of mutations) {
      mutation.addedNodes.forEach((node) => {
        if (!(node instanceof Element)) return;
        const overlay = node.matches(".modal-overlay") ? node : node.querySelector(".modal-overlay");
        // laisse React finir de monter le contenu (autoFocus compris)
        if (overlay) requestAnimationFrame(() => focusModal(overlay));
      });
    }
  });
  observer.observe(document.body, { childList: true, subtree: true });
}
