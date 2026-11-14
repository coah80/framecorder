import tailwind from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { fileURLToPath } from "node:url"
import { defineConfig } from "vite"

// the service serves dist/ flat: index.html, login.css and the login page's
// fonts at the top, everything the app loads under /assets/<name>. nothing is
// inlined, since the page's content security policy allows no inline script or style
export default defineConfig(({ mode }) => ({
  plugins: [react(), tailwind()],
  // the kit checks this to keep its development warnings out of a build
  define: { "process.env.NODE_ENV": JSON.stringify(mode) },
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  build: { assetsInlineLimit: 0, modulePreload: { polyfill: false }, sourcemap: false },
}))
