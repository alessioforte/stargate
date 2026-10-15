import { StrictMode, type ReactNode } from "react";
import { MantineProvider } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import { defaultColorScheme, theme } from "./theme";

/** Providers shared by the browser entry and the build-time prerender. */
export function AppProviders({ children }: { children: ReactNode }) {
  return (
    <StrictMode>
      <MantineProvider theme={theme} defaultColorScheme={defaultColorScheme}>
        <Notifications position="bottom-right" />
        {children}
      </MantineProvider>
    </StrictMode>
  );
}
