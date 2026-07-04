import { fileURLToPath, URL } from "node:url";
import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

const appRoot = fileURLToPath(new URL(".", import.meta.url));
const envPrefix = "VITE_";

function normalizeBasePath(value: string | undefined) {
  const trimmed = value?.trim().replace(/^\/+|\/+$/g, "");
  return trimmed ? `/${trimmed}/` : "/";
}

// https://vite.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, appRoot, envPrefix);

  return {
    base: normalizeBasePath(env.VITE_BASE_PATH ?? "/auth"),
    envDir: appRoot,
    plugins: [react()],
    envPrefix,
    build: {
      outDir: "../../.stargate/apps/auth",
      emptyOutDir: true,
      rolldownOptions: {
        output: {
          codeSplitting: {
            groups: [
              {
                name: "react",
                test: /node_modules[\\/](react|react-dom|scheduler)[\\/]/,
                priority: 30,
              },
              {
                name: "mantine",
                test: /node_modules[\\/]@mantine[\\/]/,
                priority: 20,
              },
              {
                name: "vendor",
                test: /node_modules[\\/]/,
                priority: 10,
              },
            ],
          },
        },
      },
    },
    resolve: {
      alias: {
        "@": appRoot,
      },
    },
    server: {
      port: 3010,
    },
    preview: {
      port: 3010,
    },
  };
});
