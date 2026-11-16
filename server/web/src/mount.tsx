// SPDX-License-Identifier: MIT
import { createRoot } from "react-dom/client";
import { App } from "./app";
import "./style.css";
export function mount() {
  createRoot(document.getElementById("root")!).render(<App />);
}
