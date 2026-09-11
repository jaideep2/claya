import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Shell } from "./Shell";
import "./shell.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Shell />
  </StrictMode>
);
