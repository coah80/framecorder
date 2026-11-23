// framecorder's installer for the steam frame. site/install downloads it and
// runs it: install or update, the panels' permission, or removing it.

import * as headset from "./headset"
import { Screen, type Tone } from "./screen"

const quit = { value: "quit", name: "quit", description: "" } as const
const done = { value: "done", name: "done", description: "" } as const

async function main(screen: Screen): Promise<void> {
  if (process.platform !== "linux" || process.arch !== "arm64") {
    await screen.ask("this is for the steam frame", `this machine is ${process.platform} ${process.arch}.`, [quit], "bad")
    return
  }
  if (!headset.installed()) {
    const pick = await screen.ask(
      "install framecorder?",
      "records what the frame's panels show. about 5 MB.\n\nit needs your password once, to read the panels.",
      [{ value: "install", name: "install", description: "" }, quit],
    )
    if (pick === "install") await install(screen)
    return
  }

  const pick = await screen.ask("framecorder is installed", "", [
    { value: "update", name: "update", description: "" },
    { value: "remove", name: "remove", description: "" },
    quit,
  ])
  if (pick === "update") await install(screen)
  if (pick === "remove") await remove(screen)
}

async function install(screen: Screen): Promise<void> {
  const auth = await authorize(screen)
  if (auth !== "ok") {
    if (auth === "cancelled") {
      const why = headset.hasPassword()
        ? "it needs your password to read the panels. run this again when you're ready."
        : "framecorder won't work without a sudo password. set one in steam under developer mode, then run this again."
      await screen.ask("nothing was installed", why, [done], "bad")
    }
    return
  }
  await headset.install((status) => screen.busy("installing framecorder", status))
  if (!headset.unlocked()) {
    if (!(await headset.authorized()) && (await authorize(screen)) !== "ok") {
      await screen.ask(
        "panels are still locked",
        "framecorder is installed, but it needs your password to read the panels. run this again when you're ready.",
        [done],
        "bad",
      )
      return
    }
    screen.busy("installing framecorder", "unlocking the panels")
    await headset.unlock()
  }
  await screen.ask(
    "framecorder is ready",
    "open the steamvr dashboard in the headset: framecorder is a tab there.\n\nrun this again to update or remove it.",
    [done],
    "good",
  )
}

async function ensurePassword(screen: Screen): Promise<boolean> {
  if (headset.hasPassword()) return true
  let heading = "set a password first"
  let tone: Tone = "normal"
  while (!headset.hasPassword()) {
    const pick = await screen.ask(
      heading,
      "steamos doesn't have a password yet, and framecorder won't work without one.\n\nin steam: settings -> system -> developer mode, then settings -> developer -> user password.\n\nonce it's set, check again, or set one here in the terminal.",
      [
        { value: "check", name: "check again", description: "after setting it in steam" },
        { value: "set", name: "set a password here", description: "runs passwd in this terminal" },
        quit,
      ],
      tone,
    )
    if (pick === "quit") return false
    if (pick === "set") {
      const res = await screen.handOver(headset.setPassword)
      if (headset.hasPassword()) return true
      heading = "no password was set"
      tone = "bad"
      if (res.said) {
        const again = await screen.ask("no password was set", res.said, [{ value: "retry", name: "back", description: "" }, quit], "bad")
        if (again !== "retry") return false
      }
      continue
    }
    if (!headset.hasPassword()) {
      heading = "still no password set"
      tone = "bad"
    }
  }
  return true
}

async function authorize(screen: Screen): Promise<"ok" | "cancelled" | "blocked"> {
  if (headset.sandboxed()) {
    await screen.ask(
      "this terminal is sandboxed",
      "sudo can't run inside a sandboxed terminal, so framecorder can't unlock the panels here.\n\nopen konsole or ssh into the frame and run the install command there.",
      [done],
      "bad",
    )
    return "blocked"
  }
  if (!(await ensurePassword(screen))) return "cancelled"
  if (await headset.authorized()) return "ok"
  for (;;) {
    const res = await screen.handOver(headset.authorize)
    if (res.ok) return "ok"
    const body = res.said ? `${res.said}\n\ntry your password again?` : "try your password again?"
    const pick = await screen.ask("that didn't work", body, [{ value: "retry", name: "try again", description: "" }, quit], "bad")
    if (pick !== "retry") return "cancelled"
  }
}

async function remove(screen: Screen): Promise<void> {
  const pick = await screen.ask("remove framecorder?", "your videos stay in ~/Videos/framecorder.", [
    { value: "remove", name: "remove", description: "programs, settings and pairings" },
    { value: "cancel", name: "cancel", description: "" },
  ])
  if (pick !== "remove") return
  screen.busy("removing framecorder", "removing")
  await headset.remove()
  await screen.ask("framecorder is gone", "your videos are still in ~/Videos/framecorder.", [done], "good")
}

const screen = await Screen.open()
// konsole closed: there's no one left to ask, and the terminal's gone
process.on("SIGHUP", () => process.exit(129))
try {
  await main(screen)
} catch (e) {
  await screen.ask("something went wrong", e instanceof Error ? e.message : String(e), [done], "bad")
} finally {
  screen.close()
}
