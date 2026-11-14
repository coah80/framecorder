// the login page is served to anyone, so it gets its own copies of the two
// font files it uses, at the top of dist/ where the service hands them out
import { copyFileSync, mkdirSync } from "node:fs"

mkdirSync("public", { recursive: true })
for (const [pkg, file] of [
  ["montserrat", "montserrat-latin-800-normal.woff2"],
  ["poppins", "poppins-latin-400-normal.woff2"],
  ["poppins", "poppins-latin-500-normal.woff2"],
]) {
  copyFileSync(`node_modules/@fontsource/${pkg}/files/${file}`, `public/${file}`)
}
