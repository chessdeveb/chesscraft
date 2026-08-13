import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { GamePickerWindow } from "./GamePickerWindow";

const isGamePicker = new URLSearchParams(window.location.search).get("window") === "game-picker";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {isGamePicker ? <GamePickerWindow /> : <App />}
  </React.StrictMode>,
);
