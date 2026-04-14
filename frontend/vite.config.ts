import { defineConfig } from "vite";
import solid from "vite-plugin-solid";
import { vanillaExtractPlugin } from "@vanilla-extract/vite-plugin";

export default defineConfig({
  plugins: [vanillaExtractPlugin(), solid()],
  base: "/static/",
  build: {
    outDir: "dist",
    rollupOptions: {
      input: {
        index: "src/index.tsx",
        reviews: "src/reviews.tsx",
      },
      output: {
        entryFileNames: "[name].js",
        assetFileNames: "[name][extname]",
      },
    },
  },
});
