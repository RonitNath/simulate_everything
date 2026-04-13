import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { vanillaExtractPlugin } from "@vanilla-extract/vite-plugin";

export default defineConfig({
  plugins: [vanillaExtractPlugin(), solid()],
  build: {
    outDir: "dist",
    rollupOptions: {
      input: "src/index.tsx",
      output: {
        entryFileNames: "index.js",
        assetFileNames: "[name][extname]",
      },
    },
  },
});
