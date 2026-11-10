import { build } from "esbuild";
import { render } from "@react-email/components";
import React from "react";
import { createRequire } from "node:module";
import { mkdir, rm, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const appRoot = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
const emailsDir = path.join(appRoot, "emails");
const buildDir = path.join(appRoot, ".react-email-build");
const outDir = path.resolve(appRoot, "../../.stargate/transactional");
const require = createRequire(import.meta.url);

async function loadModule(entry, name) {
  const outfile = path.join(buildDir, `${name}.cjs`);
  await build({
    entryPoints: [entry],
    outfile,
    bundle: true,
    platform: "node",
    format: "cjs",
    jsx: "automatic",
    packages: "external",
    logLevel: "silent",
  });
  return require(outfile);
}

async function main() {
  await rm(buildDir, { recursive: true, force: true });
  await mkdir(buildDir, { recursive: true });
  await rm(outDir, { recursive: true, force: true });
  await mkdir(outDir, { recursive: true });

  const i18n = await loadModule(
    path.join(emailsDir, "_components", "i18n.ts"),
    "i18n",
  );
  const rendered = [];

  for (const templateId of i18n.templateIds) {
    const module = await loadModule(
      path.join(emailsDir, `${templateId}.tsx`),
      templateId,
    );
    if (typeof module.default !== "function") {
      throw new Error(`emails/${templateId}.tsx has no default component`);
    }

    for (const locale of i18n.locales) {
      const html = await render(
        React.createElement(module.default, { locale }),
      );
      const filename = `${templateId}.${locale}.html`;
      await writeFile(path.join(outDir, filename), html);
      rendered.push(filename);
    }
  }

  const subjects = i18n.buildSubjects();
  await writeFile(
    path.join(outDir, "subjects.json"),
    `${JSON.stringify(subjects, null, 2)}\n`,
  );

  console.log(
    `Rendered ${rendered.length} localized templates and subjects.json to ${outDir}`,
  );
}

try {
  await main();
} finally {
  await rm(buildDir, { recursive: true, force: true });
}
