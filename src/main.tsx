import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { GamePickerWindow } from "./GamePickerWindow";
import { HomeWindow } from "./HomeWindow";
import { PositionEditorWindow } from "./PositionEditorWindow";
import { AboutWindow } from "./AboutWindow";
import "./i18n";
import { installModalFocus } from "./modalFocus";

installModalFocus();

const windowParam = new URLSearchParams(window.location.search).get("window");

function Root() {
  if (windowParam === "game-picker") return <GamePickerWindow />;
  if (windowParam === "position-editor") return <PositionEditorWindow />;
  if (windowParam === "about") return <AboutWindow />;
  if (windowParam === "game") return <App />;
  return <HomeWindow />;
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Root />
  </React.StrictMode>,
);
