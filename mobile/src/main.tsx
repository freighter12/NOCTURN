import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "./styles.css";
import { isNative } from "./native";

// On the iPhone the theme background is drawn natively behind a see-through page.
if (isNative) document.documentElement.classList.add("native");

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
