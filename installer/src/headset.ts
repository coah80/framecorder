// what the installer does on the headset: get the release, check it, hand it
// to framecorder-setup, and the one permission, which needs root.

import { spawnSync } from "node:child_process"
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync, writeSync } from "node:fs"
import { homedir, tmpdir } from "node:os"
import { join } from "node:path"

// FRAMECORDER_DL points it at another release, for testing one
const DEFAULT_DL = "https://framecorder.coah80.com/dl"
const DL = process.env.FRAMECORDER_DL ?? DEFAULT_DL
const HOME = homedir()
const BIN = join(HOME, ".local/bin")
const SHARE = join(HOME, ".local/share/framecorder")
const RECORDER = join(BIN, "framecorder")

export const installed = (): boolean => existsSync(join(BIN, "framecorder-ui"))

// the panel helper, which holds the permission so updates don't take it away
const HELPER = join(HOME, ".local/lib/framecorder/framecorder-grab")

/** Whether the panels are unlocked: the panel helper's in place, or (older
 * installs) the recorder has the permission itself. */
export function unlocked(): boolean {
  const has = (path: string) => spawnSync("getcap", [path], { encoding: "utf8" }).stdout?.includes("cap_sys_admin") ?? false
  return has(HELPER) || has(RECORDER)
}

export function hasPassword(): boolean {
  const status = spawnSync("passwd", ["-S"], { encoding: "utf8" })
  if (status.status === 0) {
    const state = status.stdout.trim().split(/\s+/)[1]
    if (state === "P") return true
    if (state === "NP" || state === "L") return false
  }
  const steamos = spawnSync("steamos-passwd", ["--has-password"], { stdio: "ignore" })
  if (!steamos.error && steamos.status !== null) return steamos.status === 0
  return false
}

export function sandboxed(): boolean {
  try {
    return /^NoNewPrivs:\s*1$/m.test(readFileSync("/proc/self/status", "utf8"))
  } catch {
    return false
  }
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
    // updates come from the same place (the dev builds, say), see SOURCE in setup.rs
    if (DL === DEFAULT_DL) rmSync(join(SHARE, "source"), { force: true })
    else writeFileSync(join(SHARE, "source"), `${DL}\n`)
  } finally {
    rmSync(work, { recursive: true, force: true })
  }
}

export async function remove(): Promise<void> {
  await run(join(BIN, "framecorder-ui"), ["--uninstall"])
}

export type TalkResult = { ok: boolean; said: string }

async function talk(program: string, args: string[], banner?: string): Promise<TalkResult> {
  if (banner) writeSync(1, banner)
  let proc
  try {
    proc = Bun.spawn([program, ...args], { stdin: "inherit", stdout: "inherit", stderr: "pipe" })
  } catch (e) {
    return { ok: false, said: e instanceof Error ? e.message : String(e) }
  }
  const chunks: Uint8Array[] = []
  const reader = proc.stderr.getReader()
  const drain = (async () => {
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      writeSync(2, value)
      chunks.push(value)
    }
  })()
  const [code] = await Promise.all([proc.exited, drain])
  const said = Buffer.concat(chunks).toString("utf8").trim().split("\n").slice(-4).join("\n")
  return { ok: code === 0, said }
}

export function setPassword(): Promise<TalkResult> {
  return talk("passwd", [], "\npick a password for steamos. you'll type it twice, nothing shows while you type.\n\n")
}

export async function authorized(): Promise<boolean> {
  const proc = Bun.spawn(["sudo", "-n", "-v"], { stdin: "ignore", stdout: "ignore", stderr: "ignore" })
  return (await proc.exited) === 0
}

export function authorize(): Promise<TalkResult> {
  return talk("sudo", ["-v"], "\nyour password, so framecorder can record the panels:\n\n")
}

/** Puts the panel helper in place, with the password from `authorize`.
 * framecorder-setup does it, see setup::unlock. */
export async function unlock(): Promise<void> {
  await run(join(BIN, "framecorder-setup"), ["--unlock"])
}
