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
      "it records what the frame's panels show, from a tab in the steamvr dashboard. clips, and sync to your phone or computer too. about 5 MB, and it updates itself.",
      [{ value: "install", name: "install", description: "takes a few seconds" }, quit],
    )
    if (pick === "install") await install(screen)
    return
  }

  const choices = [
    { value: "update", name: "update", description: "get the latest version now (it also updates itself)" },
    ...(headset.unlocked() ? [] : [{ value: "unlock", name: "unlock the panels", description: "sharper, any shape, costs the game nothing" }]),
    { value: "remove", name: "remove", description: "take framecorder off the headset" },
    quit,
  ]
  const pick = await screen.ask("framecorder is installed", "what do you want to do?", choices)
  if (pick === "update") await install(screen)
  if (pick === "unlock") await offerUnlock(screen, true)
  if (pick === "remove") await remove(screen)
}

async function install(screen: Screen): Promise<void> {
  await headset.install((status) => screen.busy("installing framecorder", status))
  await offerUnlock(screen, false)
  await screen.ask(
    "framecorder is ready",
    "put the headset on and open the steamvr dashboard: there's a framecorder tab with the record button.\n\nit updates itself from now on. run this again to remove it.",
    [done],
    "good",
  )
}

/** The permission that lets the recorder read the panels. Optional. */
async function offerUnlock(screen: Screen, asked: boolean): Promise<void> {
  if (headset.unlocked()) return
  if (!asked) {
    const pick = await screen.ask(
      "one more thing: unlock the panels?",
      "without it, framecorder records steamvr's view: 16:9 of the left eye, and it costs the game a little gpu.\n\nwith it: any shape (16:9, 1:1, 9:16, both eyes), sharper, and the game doesn't notice. it's one permission on the recorder, and it takes your password once.",
      [
        { value: "unlock", name: "unlock", description: "asks for your password" },
        { value: "skip", name: "skip", description: "record steamvr's view for now" },
      ],
    )
    if (pick === "skip") return
  }
  if (!headset.hasPassword()) {
    const pick = await screen.ask(
      "pick a password first",
      "steamos doesn't have a password yet, and this needs one. remember it: steamos asks for it for things like this.",
      [
        { value: "set", name: "set a password", description: "you'll type it twice" },
        { value: "skip", name: "skip", description: "record steamvr's view for now" },
      ],
    )
    if (pick === "skip") return
    if (!screen.handOver(headset.setPassword)) {
      await screen.ask("no password was set", "so the panels stay locked. run this again any time.", [done], "bad")
      return
    }
  }
  if (!screen.handOver(headset.unlock)) {
    await screen.ask("that didn't work", "it records steamvr's view for now. run this again to try again.", [done], "bad")
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
try {
  await main(screen)
} catch (e) {
  await screen.ask("something went wrong", e instanceof Error ? e.message : String(e), [done], "bad")
} finally {
  screen.close()
}
