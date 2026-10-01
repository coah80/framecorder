// the installer's one screen: a panel with a heading, some words, and either
// a list to pick from or a spinner. catppuccin mocha, like the tab and the app.

import {
  BoxRenderable,
  SelectRenderable,
  SelectRenderableEvents,
  TextAttributes,
  TextRenderable,
  createCliRenderer,
  type CliRenderer,
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
    const renderer = await createCliRenderer({ exitOnCtrlC: true })
    renderer.setBackgroundColor(color.base)
    return new Screen(renderer)
  }

  /** Shows the words and waits for one of the choices. */
  ask<T extends string>(heading: string, body: string, choices: Choice<T>[], tone: Tone = "normal"): Promise<T> {
    const panel = this.show(heading, body, tone)
    const list = new SelectRenderable(this.renderer, {
      options: choices,
      // a name and a description each
      height: choices.length * 2,
      width: "100%",
      backgroundColor: color.base,
      focusedBackgroundColor: color.base,
      selectedBackgroundColor: color.surface,
      textColor: color.subtext,
      focusedTextColor: color.subtext,
      selectedTextColor: color.mauve,
      descriptionColor: color.overlay,
      selectedDescriptionColor: color.subtext,
    })
    panel.add(list)
    panel.add(this.hint("↑↓ choose · enter to go · ctrl+c to quit"))
    list.focus()
    return new Promise((resolve) => {
      list.on(SelectRenderableEvents.ITEM_SELECTED, (_index: number, option: Choice<T>) => resolve(option.value))
    })
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
  handOver<R>(run: () => R): R {
    this.stopSpinner()
    this.renderer.suspend()
    try {
      return run()
    } finally {
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
