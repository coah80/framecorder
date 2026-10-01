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
      "it records what the frame's panels show, from a tab in the steamvr dashboard. clips, and sync to your phone or computer too. about 5 MB, and it updates itself.\n\nit asks for your password first, so it can record the panels themselves: any shape, sharper, and the game doesn't notice.",
      [{ value: "install", name: "install", description: "asks for your password, then does the rest" }, quit],
    )
    if (pick === "install") await install(screen)
    return
  }

  const locked = !headset.unlocked()
  const choices = [
    { value: "update", name: "update", description: "get the latest version now (it also updates itself)" },
    ...(locked ? [{ value: "unlock", name: "unlock the panels", description: "sharper, any shape, costs the game nothing" }] : []),
    { value: "remove", name: "remove", description: "take framecorder off the headset" },
    quit,
  ]
  const pick = await screen.ask("framecorder is installed", "what do you want to do?", choices)
  // an update can replace the recorder, which takes its permission, so the
  // password comes first then too
  if (pick === "update") await install(screen)
  if (pick === "unlock" && (await authorize(screen)) && (await unlock(screen))) {
    await screen.ask("the panels are unlocked", "framecorder records what the panels show now, any shape you pick in the tab.", [done], "good")
  }
  if (pick === "remove") await remove(screen)
}

/** Password first, then everything else runs on its own. */
async function install(screen: Screen): Promise<void> {
  const authorized = await authorize(screen)
  await headset.install((status) => screen.busy("installing framecorder", status))
  if (authorized) await unlock(screen)
  const view = headset.unlocked()
    ? "it records the panels."
    : "it records steamvr's view for now. run this again and pick unlock the panels for the full thing."
  await screen.ask(
    "framecorder is ready",
    `put the headset on and open the steamvr dashboard: there's a framecorder tab with the record button. ${view}\n\nit updates itself from now on. run this again to remove it.`,
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
      "steamos doesn't have a password yet, and recording the panels needs one. remember it: steamos asks for it for things like this.",
      [
        { value: "set", name: "set a password", description: "you'll type it twice" },
        { value: "skip", name: "skip", description: "record steamvr's view for now" },
      ],
    )
    if (pick === "skip") return false
    if (!(await screen.handOver(headset.setPassword))) {
      await screen.ask("no password was set", "so it'll record steamvr's view for now. run this again any time.", [done], "bad")
      return false
    }
  }
  while (!(await screen.handOver(headset.authorize))) {
    const pick = await screen.ask("no password", "that didn't work. try again, or go on and record steamvr's view for now.", [
      { value: "retry", name: "try again", description: "" },
      { value: "skip", name: "go on without it", description: "record steamvr's view for now" },
    ], "bad")
    if (pick === "skip") return false
  }
  return true
}

/** The permission that lets the recorder read the panels. */
async function unlock(screen: Screen): Promise<boolean> {
  screen.busy("framecorder", "unlocking the panels")
  try {
    await headset.unlock()
    return true
  } catch {
    await screen.ask("couldn't unlock the panels", "it records steamvr's view for now. run this again to try again.", [done], "bad")
    return false
  }
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
