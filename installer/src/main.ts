// framecorder's installer for the steam frame. site/install downloads it and
// runs it: install or update, the panels' permission, or removing it.

import * as headset from "./headset"
import { Screen } from "./screen"

const quit = { value: "quit", name: "quit", description: "leave things as they are" } as const
const done = { value: "done", name: "done", description: "" } as const

async function main(screen: Screen): Promise<void> {
  if (process.platform !== "linux" || process.arch !== "arm64") {
    await screen.ask("this is for the steam frame", `this machine is ${process.platform} ${process.arch}.`, [quit], "bad")
    return
  }
  if (!headset.installed()) {
    const pick = await screen.ask(
      "install framecorder?",
      "it records what the frame's panels show, from a tab in the steamvr dashboard. clips, and sync to your phone or computer too. about 5 MB, and it updates itself.\n\nit needs your password once: reading the panels takes a permission only steamos can give.",
      [{ value: "install", name: "install", description: "asks for your password, then does the rest" }, quit],
    )
    if (pick === "install") await install(screen)
    return
  }

  const pick = await screen.ask("framecorder is installed", "what do you want to do?", [
    { value: "update", name: "update", description: "get the latest version now (it also updates itself)" },
    { value: "remove", name: "remove", description: "take framecorder off the headset" },
    quit,
  ])
  if (pick === "update") await install(screen)
  if (pick === "remove") await remove(screen)
}

/** Password first, then everything else runs on its own. No password, no
 * install: the panels are what framecorder records. */
async function install(screen: Screen): Promise<void> {
  if (!(await authorize(screen))) {
    await screen.ask(
      "nothing was installed",
      "framecorder needs your password to read the panels, that's what it records. run this again when you're ready.",
      [done],
      "bad",
    )
    return
  }
  await headset.install((status) => screen.busy("installing framecorder", status))
  // the install itself puts the panel helper in place with the password,
  // this is in case it couldn't
  if (!headset.unlocked()) {
    screen.busy("installing framecorder", "unlocking the panels")
    await headset.unlock()
  }
  await screen.ask(
    "framecorder is ready",
    "put the headset on and open the steamvr dashboard: there's a framecorder tab with the record button.\n\nit updates itself from now on. run this again to remove it.",
    [done],
    "good",
  )
}

/** Gets the password, before anything else happens. Helps set one if
 * steamos doesn't have one yet. */
async function authorize(screen: Screen): Promise<boolean> {
  if (!headset.hasPassword()) {
    const pick = await screen.ask(
      "pick a password first",
      "steamos doesn't have a password yet, and framecorder needs one. remember it: steamos asks for it for things like this.",
      [{ value: "set", name: "set a password", description: "you'll type it twice" }, quit],
    )
    if (pick !== "set") return false
    while (!(await screen.handOver(headset.setPassword))) {
      const again = await screen.ask("no password was set", "try again?", [{ value: "retry", name: "try again", description: "" }, quit], "bad")
      if (again !== "retry") return false
    }
  }
  while (!(await screen.handOver(headset.authorize))) {
    const pick = await screen.ask("that didn't work", "try your password again?", [{ value: "retry", name: "try again", description: "" }, quit], "bad")
    if (pick !== "retry") return false
  }
  return true
}

async function remove(screen: Screen): Promise<void> {
  const pick = await screen.ask("remove framecorder?", "your videos stay in ~/Videos/framecorder.", [
    { value: "remove", name: "remove", description: "programs, settings and pairings" },
    { value: "cancel", name: "cancel", description: "keep it" },
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
