import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  base: "/depthdeck/",
  plugins: [react()],
  worker: { format: "es" },
});
