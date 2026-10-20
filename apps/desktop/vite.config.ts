import { resolve } from "node:path";
import { defineConfig } from "vite";

// Each window of the app is its own page.
export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Rust sources and build output (including bundled binaries) aren't frontend files.
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2022",
    rollupOptions: {
      input: {
        wheel: resolve(__dirname, "wheel.html"),
        settings: resolve(__dirname, "settings.html"),
        editor: resolve(__dirname, "editor.html"),
        float: resolve(__dirname, "float.html"),
      },
    },
  },
});
