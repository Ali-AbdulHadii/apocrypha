import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { LightboxProvider } from "./components/Preview";
import { ToastProvider } from "./components/ui";
import { bootAppearance } from "./lib/appearance";
import { ThemeProvider } from "./lib/theme";
import "./styles/theme.css";
import "./styles/app.css";
// Screen-specific sheets, loaded after app.css so they can build on its classes
// without having to out-specify them.
import "./styles/order.css";
import "./styles/settings.css";
import { ErrorBoundary } from "./components/ErrorBoundary";

// Apply saved appearance before the first paint so there is no flash of the
// default theme on startup.
bootAppearance();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ThemeProvider>
      <ToastProvider>
        <LightboxProvider>
          {/* Outermost so nothing can white-screen the window. Inner
              boundaries can be added per screen later; this one is the
              difference between a message and an empty window. */}
          <ErrorBoundary>
            <App />
          </ErrorBoundary>
        </LightboxProvider>
      </ToastProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
