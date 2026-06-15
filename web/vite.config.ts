import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// Served under /_admin/ inside the single binary; build output is embedded
// into the Rust binary via rust-embed (resources/admin).
export default defineConfig({
  base: "/_admin/",
  plugins: [react(), tailwindcss()],
  build: {
    outDir: "../resources/admin",
    emptyOutDir: true,
  },
  server: {
    proxy: {
      "/api": "http://127.0.0.1:21114",
    },
  },
});
