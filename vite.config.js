import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
// Tauri-specific config: fixed port + clear-screen disabled so logs from
// Rust + Vite are both visible in the same terminal during dev.
export default defineConfig({
    plugins: [react()],
    clearScreen: false,
    server: {
        port: 1420,
        strictPort: true,
        host: "localhost",
        watch: {
            ignored: ["**/src-tauri/**"],
        },
    },
    envPrefix: ["VITE_", "TAURI_ENV_"],
    build: {
        target: "es2021",
        minify: "esbuild",
        sourcemap: true,
    },
});
