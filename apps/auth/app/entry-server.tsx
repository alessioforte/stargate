import { renderToStaticMarkup, renderToString } from "react-dom/server";
import { ColorSchemeScript } from "@mantine/core";
import {
  createStaticHandler,
  createStaticRouter,
  StaticRouterProvider,
} from "react-router";
import { AppProviders } from "./providers";
import { routeObjects } from "./route-objects";
import { defaultColorScheme } from "./theme";

/**
 * Build-time render of one route, used by scripts/prerender.mjs. `url` is
 * the full path including the app base path (e.g. `/auth/login`).
 */
export async function render(url: string, basename: string) {
  const handler = createStaticHandler(routeObjects, { basename });
  const context = await handler.query(new Request(`http://localhost${url}`));
  if (context instanceof Response) {
    throw new Error(`prerender of ${url} redirected`);
  }

  const router = createStaticRouter(handler.dataRoutes, context);
  // No loaders, so there is no hydration data to embed.
  return renderToString(
    <AppProviders>
      <StaticRouterProvider router={router} context={context} hydrate={false} />
    </AppProviders>,
  );
}

/**
 * Inline script that applies the saved color scheme before first paint. If a
 * Content-Security-Policy is ever added, it needs this script's hash.
 */
export function colorSchemeScript() {
  return renderToStaticMarkup(
    <ColorSchemeScript defaultColorScheme={defaultColorScheme} />,
  );
}
