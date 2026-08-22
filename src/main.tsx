import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { BasesWindow } from "./BasesWindow";
import { GamePickerWindow } from "./GamePickerWindow";
import { HomeWindow } from "./HomeWindow";

const windowParam = new URLSearchParams(window.location.search).get("window");

function Root() {
  if (windowParam === "game-picker") return <GamePickerWindow />;
  if (windowParam === "bases-paths") return <BasesWindow />;
  if (windowParam === "game") return <App />;
  return <HomeWindow />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
