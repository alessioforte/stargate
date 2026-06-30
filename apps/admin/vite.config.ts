import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  envPrefix: ["VITE_", "PUBLIC_RUNTIME_", "API_"],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL(".", import.meta.url)),
    },
  },
  server: {
    port: 3011,
  },
  preview: {
    port: 3011,
  },
});
