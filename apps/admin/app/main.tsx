import { createTheme, MantineProvider } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import { createRoot } from "react-dom/client";
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";
import "./index.css";
import App from "./app.tsx";

const theme = createTheme({
  primaryColor: "yellow",
  defaultRadius: "sm",
});

createRoot(document.getElementById("root")!).render(
  <MantineProvider theme={theme} defaultColorScheme="auto">
    <Notifications position="bottom-right" />
    <App />
  </MantineProvider>,
);
