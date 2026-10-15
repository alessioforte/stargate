// Prerenders the login route into `login.html` after `vite build`, so the
// page works without JavaScript (the form posts natively to /oauth/login)
// and hydrates when JavaScript runs. Also inlines Mantine's color scheme
// script into every HTML entry to avoid a flash of the wrong theme.
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

const loginPath = `${basename === "/" ? "" : basename}/login`;
const markup = await render(loginPath, basename);
fs.writeFileSync(indexPath, withScheme);
fs.writeFileSync(
  path.join(outDir, "login.html"),
  withScheme.replace(ROOT_PLACEHOLDER, `<div id="root">${markup}</div>`),
);
fs.rmSync(ssrOutDir, { recursive: true, force: true });

console.log(
  `prerendered ${loginPath} -> ${path.relative(appRoot, outDir)}/login.html`,
);
