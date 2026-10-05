import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { legacy } from "./lib/routes";
import "./style.css";
import { ThemeProvider } from "./lib/theme";

const moved = legacy(location.hash);
if (moved) history.replaceState(null, "", moved);

// The prerendered markup is for crawlers and the first paint; the visitor's own render replaces it.
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </StrictMode>,
);
