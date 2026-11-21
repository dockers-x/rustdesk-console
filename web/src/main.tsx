import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "./styles.css";
import "./i18n";
import { initTheme } from "./lib/theme";
import App from "./App";

// Apply the saved/system light-dark mode before the first paint.
initTheme();

// Keyboard navigation should switch UI states immediately, even when a dialog
// moves focus from its trigger to a different element.
document.addEventListener("keydown", () => {
  if (!document.documentElement.hasAttribute("data-keyboard")) {
    document.documentElement.dataset.keyboard = "";
  }
}, { capture: true });
document.addEventListener("pointerdown", () => {
  delete document.documentElement.dataset.keyboard;
}, { capture: true });

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
});

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <App />
    </QueryClientProvider>
  </React.StrictMode>,
);
