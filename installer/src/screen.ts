// the installer's one screen: a panel with a heading, some words, and either
// a list to pick from or a spinner. catppuccin mocha, like the tab and the app.

import {
  BoxRenderable,
  TextAttributes,
  TextRenderable,
  createCliRenderer,
  type CliRenderer,
  type KeyEvent,
} from "@opentui/core"

export const color = {
  base: "#1e1e2e",
  text: "#cdd6f4",
  subtext: "#a6adc8",
  overlay: "#7f849c",
  surface: "#313244",
  mauve: "#cba6f7",
  green: "#a6e3a1",
  red: "#f38ba8",
}

export interface Choice<T extends string> {
  value: T
  name: string
  description: string
}

export type Tone = "normal" | "good" | "bad"

const SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]
const TONES: Record<Tone, string> = { normal: color.mauve, good: color.green, bad: color.red }

export class Screen {
  private panel: BoxRenderable | null = null
  private spinning: ReturnType<typeof setInterval> | null = null

  private constructor(private renderer: CliRenderer) {}

  static async open(): Promise<Screen> {
    const renderer = await createCliRenderer({ exitOnCtrlC: true, useMouse: true })
    renderer.setBackgroundColor(color.base)
    return new Screen(renderer)
  }

  /** Shows the words and waits for one of the choices: a click (there's no
   * keyboard in steamvr), or the arrow keys and enter. */
  ask<T extends string>(heading: string, body: string, choices: Choice<T>[], tone: Tone = "normal"): Promise<T> {
    const panel = this.show(heading, body, tone)
    return new Promise((resolve) => {
      let active = 0
      let picked = false
      const buttons = choices.map((choice, i) => this.button(choice, () => highlight(i), () => pick(i)))
      const paint = () =>
        buttons.forEach(({ box, name }, i) => {
          box.borderColor = i === active ? color.mauve : color.surface
          box.backgroundColor = i === active ? color.surface : color.base
          name.fg = i === active ? color.mauve : color.text
        })
      const highlight = (i: number) => {
        active = i
        paint()
      }
      const pick = (i: number) => {
        if (picked) return
        picked = true
        this.renderer.keyInput.off("keypress", keys)
        // let the click finish going through opentui first: handing the
        // terminal over in the middle of it leaves the terminal stuck
        setTimeout(() => resolve(choices[i].value), 50)
      }
      const keys = (key: KeyEvent) => {
        const step = { up: -1, left: -1, down: 1, right: 1, tab: key.shift ? -1 : 1 }[key.name]
        if (step) highlight((active + step + choices.length) % choices.length)
        if (key.name === "return" || key.name === "enter") pick(active)
      }
      for (const { box } of buttons) panel.add(box)
      panel.add(this.hint(choices.length > 1 ? "click one, or use the arrow keys and enter" : "click it, or press enter"))
      paint()
      this.renderer.keyInput.on("keypress", keys)
    })
  }

  private button(choice: Choice<string>, over: () => void, up: () => void): { box: BoxRenderable; name: TextRenderable } {
    const box = new BoxRenderable(this.renderer, {
      width: "100%",
      border: true,
      borderStyle: "rounded",
      paddingX: 2,
      flexDirection: "row",
      gap: 2,
      onMouseOver: over,
      onMouseUp: up,
    })
    const name = new TextRenderable(this.renderer, { content: choice.name, attributes: TextAttributes.BOLD })
    box.add(name)
    if (choice.description) box.add(new TextRenderable(this.renderer, { content: choice.description, fg: color.overlay }))
    return { box, name }
  }

  /** Shows the words with a spinner, until the next screen. */
  busy(heading: string, status: string): void {
    const panel = this.show(heading, "")
    const line = new TextRenderable(this.renderer, { content: status, fg: color.subtext })
    panel.add(line)
    let frame = 0
    this.spinning = setInterval(() => {
      frame = (frame + 1) % SPINNER.length
      line.content = `${SPINNER[frame]}  ${status}`
    }, 80)
  }

  /** Hands the terminal to something that asks for itself, like sudo. */
  async handOver<R>(run: () => Promise<R>): Promise<R> {
    this.stopSpinner()
    this.renderer.suspend()
    // ctrl+c there cancels the prompt, it shouldn't take the installer with it
    const stay = () => {}
    process.on("SIGINT", stay)
    try {
      return await run()
    } finally {
      process.off("SIGINT", stay)
      this.renderer.resume()
    }
  }

  close(): void {
    this.stopSpinner()
    this.renderer.destroy()
  }

  private show(heading: string, body: string, tone: Tone = "normal"): BoxRenderable {
    this.stopSpinner()
    if (this.panel) this.panel.destroyRecursively()
    this.renderer.root.alignItems = "center"
    this.renderer.root.justifyContent = "center"
    const panel = new BoxRenderable(this.renderer, {
      width: Math.min(76, this.renderer.width - 2),
      border: true,
      borderStyle: "rounded",
      borderColor: color.surface,
      title: " framecorder ",
      titleColor: color.mauve,
      paddingX: 3,
      paddingY: 1,
      flexDirection: "column",
      gap: 1,
    })
    panel.add(new TextRenderable(this.renderer, { content: heading, fg: TONES[tone], attributes: TextAttributes.BOLD }))
    if (body) panel.add(new TextRenderable(this.renderer, { content: body, fg: color.text, wrapMode: "word" }))
    this.renderer.root.add(panel)
    this.panel = panel
    return panel
  }

  private hint(text: string): TextRenderable {
    return new TextRenderable(this.renderer, { content: text, fg: color.overlay })
  }

  private stopSpinner(): void {
    if (this.spinning) clearInterval(this.spinning)
    this.spinning = null
  }
}
