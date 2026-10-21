// Prerenders the pages the provider sends browsers to into `<route>.html`
// after `vite build`, so they work without JavaScript (their forms post
// natively to /oauth/login and /oauth/logout/confirm) and hydrate when
// JavaScript runs. Also inlines Mantine's color scheme script into every
// HTML entry to avoid a flash of the wrong theme.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { build, resolveConfig } from "vite";

const appRoot = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
);
const ssrOutDir = path.join(appRoot, "node_modules", ".prerender");
const ROOT_PLACEHOLDER = '<div id="root"></div>';
// Served by src/api/auth_app.rs; keep the two lists in sync.
const PRERENDERED_ROUTES = ["login", "logout", "signed-out"];

const config = await resolveConfig({ root: appRoot }, "build", "production");
const outDir = path.resolve(config.root, config.build.outDir);
const basename = config.base.replace(/\/+$/, "") || "/";

await build({
  root: appRoot,
  logLevel: "warn",
  build: {
    ssr: "app/entry-server.tsx",
    outDir: ssrOutDir,
    emptyOutDir: true,
  },
});

const { render, colorSchemeScript } = await import(
  pathToFileURL(path.join(ssrOutDir, "entry-server.js")).href
);

const indexPath = path.join(outDir, "index.html");
const template = fs.readFileSync(indexPath, "utf8");
if (!template.includes(ROOT_PLACEHOLDER)) {
  throw new Error(`${indexPath} has no ${ROOT_PLACEHOLDER}`);
}
const withScheme = template.replace("</head>", `${colorSchemeScript()}</head>`);

fs.writeFileSync(indexPath, withScheme);
for (const route of PRERENDERED_ROUTES) {
  const routePath = `${basename === "/" ? "" : basename}/${route}`;
  const markup = await render(routePath, basename);
  fs.writeFileSync(
    path.join(outDir, `${route}.html`),
    withScheme.replace(ROOT_PLACEHOLDER, `<div id="root">${markup}</div>`),
  );
  console.log(
    `prerendered ${routePath} -> ${path.relative(appRoot, outDir)}/${route}.html`,
  );
}
fs.rmSync(ssrOutDir, { recursive: true, force: true });
