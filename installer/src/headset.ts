// what the installer does on the headset: get the release, check it, hand it
// to framecorder-setup, and the one permission, which needs root.

import { spawnSync } from "node:child_process"
import { existsSync, mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { homedir, tmpdir } from "node:os"
import { join } from "node:path"

// FRAMECORDER_DL points it at another release, for testing one
const DL = process.env.FRAMECORDER_DL ?? "https://framecorder.coah80.com/dl"
const HOME = homedir()
const BIN = join(HOME, ".local/bin")
const SHARE = join(HOME, ".local/share/framecorder")
const RECORDER = join(BIN, "framecorder")
const CAPABILITY = "cap_sys_admin+ep"

export const installed = (): boolean => existsSync(join(BIN, "framecorder-ui"))

export function unlocked(): boolean {
  const out = spawnSync("getcap", [RECORDER], { encoding: "utf8" })
  return out.stdout?.includes("cap_sys_admin") ?? false
}

/** steamos starts out with no password, and sudo needs one. */
export function hasPassword(): boolean {
  const out = spawnSync("passwd", ["-S"], { encoding: "utf8" })
  return out.stdout?.split(" ")[1] === "P"
}

/** Runs a program, quietly. Throws with the end of what it said if it fails. */
async function run(program: string, args: string[]): Promise<void> {
  const proc = Bun.spawn([program, ...args], { stdout: "pipe", stderr: "pipe" })
  const [code, out, err] = await Promise.all([proc.exited, new Response(proc.stdout).text(), new Response(proc.stderr).text()])
  if (code !== 0) {
    const said = `${out}${err}`.trim().split("\n").slice(-6).join("\n")
    throw new Error(`${program.split("/").pop()} failed:\n${said}`)
  }
}

async function download(url: string): Promise<Response> {
  const res = await fetch(url).catch(() => null)
  if (!res?.ok) throw new Error("couldn't download framecorder. is the frame online?")
  return res
}

/** Downloads the latest release, checks it, and installs it. */
export async function install(status: (text: string) => void): Promise<void> {
  status("downloading framecorder")
  const [tarball, listed] = await Promise.all([
    download(`${DL}/framecorder-arm64.tar.gz`).then((r) => r.arrayBuffer()),
    download(`${DL}/framecorder-arm64.tar.gz.sha256`).then((r) => r.text()),
  ])
  const want = listed.trim().split(/\s+/)[0]
  if (new Bun.CryptoHasher("sha256").update(tarball).digest("hex") !== want) {
    throw new Error("the download got mangled on the way. run this again.")
  }

  const work = mkdtempSync(join(tmpdir(), "framecorder-"))
  try {
    writeFileSync(join(work, "release.tar.gz"), new Uint8Array(tarball))
    await run("tar", ["-xzf", join(work, "release.tar.gz"), "-C", work])
    status("installing")
    await run(join(work, "framecorder-setup"), [])
    // so the updater knows this one's installed
    writeFileSync(join(SHARE, "installed.sha256"), `${want}\n`)
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}

export async function remove(): Promise<void> {
  await run(join(BIN, "framecorder-ui"), ["--uninstall"])
}

// these two ask in the terminal themselves, so the screen hands it over first

export function setPassword(): boolean {
  console.log("\npick a password for steamos. you'll type it twice, nothing shows while you type.\n")
  return spawnSync("passwd", [], { stdio: "inherit" }).status === 0
}

export function unlock(): boolean {
  console.log("\nyour password, for sudo:\n")
  const ok = spawnSync("sudo", ["setcap", CAPABILITY, RECORDER], { stdio: "inherit" }).status === 0
  if (ok) {
    rmSync(join(SHARE, "relock"), { force: true })
    // the tab picks the panels when it starts the recorder
    spawnSync("systemctl", ["--user", "try-restart", "framecorder-ui.service"])
  }
  return ok
}
