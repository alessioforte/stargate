import { Badge, createTheme, MantineProvider } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import { createRoot } from "react-dom/client";
import App from "./app.tsx";
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/700.css";
import "./globals.css";

const theme = createTheme({
  primaryColor: "yellow",
  defaultRadius: "sm",
  components: {
    Badge: Badge.extend({
      defaultProps: {
        radius: "sm",
      },
    }),
  },
});

createRoot(document.getElementById("root")!).render(
  <MantineProvider theme={theme} defaultColorScheme="auto">
    <Notifications position="bottom-right" />
    <App />
  </MantineProvider>,
);
