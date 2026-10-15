import { createRoot, hydrateRoot } from "react-dom/client";
import App from "./app";
import { AppProviders } from "./providers";

import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";

const container = document.getElementById("root")!;
const app = (
  <AppProviders>
    <App />
  </AppProviders>
);

// Pages prerendered at build time (see scripts/prerender.mjs) arrive with
// markup in #root: hydrate it instead of replacing it.
if (container.firstElementChild) {
  hydrateRoot(container, app);
} else {
  createRoot(container).render(app);
}
