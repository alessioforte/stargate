import { MantineProvider } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import { createRoot } from "react-dom/client";
import App from "./app.tsx";
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";
import "@fontsource-variable/inter";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/700.css";
import "./globals.css";
import { theme } from "./theme";

createRoot(document.getElementById("root")!).render(
  <MantineProvider theme={theme} defaultColorScheme="auto">
    <Notifications position="bottom-right" />
    <App />
  </MantineProvider>,
);
