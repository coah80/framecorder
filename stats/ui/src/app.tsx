import { type ReactNode, useEffect, useMemo, useState } from "react"
import { Area } from "@/components/dither-kit/area"
import { AreaChart } from "@/components/dither-kit/area-chart"
import { Bar } from "@/components/dither-kit/bar"
import { BarChart } from "@/components/dither-kit/bar-chart"
import { DitherButton } from "@/components/dither-kit/button"
import { DitherGradient } from "@/components/dither-kit/gradient"
import { Grid } from "@/components/dither-kit/grid"
import type { DitherColor } from "@/components/dither-kit/palette"
import { XAxis } from "@/components/dither-kit/x-axis"
import { YAxis } from "@/components/dither-kit/y-axis"
import { APPS, type App as Surface, against, plural, ranked, type Row, rows, type Stats, weekday } from "./data"

const NAMES: Record<Surface, string> = { headset: "Headset", desktop: "Desktop", android: "Android" }
const COLOURS: Record<Surface, DitherColor> = { headset: "mauve", desktop: "sky", android: "mint" }
/** each surface keeps one colour and one texture everywhere, so they tell apart without the colour */
const TEXTURES = { headset: "solid", desktop: "hatched", android: "dotted" } as const
const INK: Record<string, string> = {
  quiet: "text-quiet",
  mauve: "text-mauve",
  sky: "text-sky",
  mint: "text-mint",
  peach: "text-peach",
  pink: "text-[#f5a6dc]",
  purple: "text-[#c8afff]",
  red: "text-red",
  orange: "text-peach",
}

/** every chart shares these, so a day sits at the same x in all of them */
const MARGINS = { left: 44, right: 10, top: 8, bottom: 6 }
const compact = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 })
const whole = (n: number) => (Number.isInteger(n) ? compact.format(n) : "")

type Load = { state: "loading" } | { state: "failed" } | { state: "ready"; stats: Stats }

async function fetchStats(): Promise<Stats | "signed-out"> {
  if (import.meta.env.DEV) return (await import("./sample")).sample()
  const reply = await fetch("/api/stats", { credentials: "same-origin", cache: "no-store" })
  // the session ended: the same address is the login page now
  if (reply.status === 404) return "signed-out"
  if (!reply.ok) throw new Error(String(reply.status))
  return reply.json()
}

export function App() {
  const [load, setLoad] = useState<Load>({ state: "loading" })
  const [span, setSpan] = useState<30 | 90>(30)
  const [at, setAt] = useState<number | null>(null)

  const refresh = () => {
    setLoad({ state: "loading" })
    fetchStats().then(
      (stats) => (stats === "signed-out" ? window.location.reload() : setLoad({ state: "ready", stats })),
      () => setLoad({ state: "failed" })
    )
  }
  useEffect(refresh, [])

  return (
    <div className="mx-auto max-w-[72rem] px-4 pb-16 sm:px-8">
      <header className="flex items-center justify-between gap-4 py-5">
        <p className="font-display text-lg font-extrabold tracking-[-0.02em] whitespace-nowrap sm:text-xl">
          framecorder<span className="text-mauve">.</span> <span className="font-bold text-quiet">stats</span>
        </p>
        <div className="flex items-center gap-2 sm:gap-3">
          <div className="flex rounded-lg border border-line p-0.5" role="group" aria-label="How far back">
            {([30, 90] as const).map((days) => (
              <button
                key={days}
                type="button"
                aria-pressed={span === days}
                onClick={() => {
                  setSpan(days)
                  setAt(null)
                }}
                className="num rounded-md px-2.5 py-1 text-xs whitespace-nowrap text-quiet transition-colors duration-150 hover:text-foreground aria-pressed:bg-surface0 aria-pressed:text-foreground"
              >
                {days}
                <span className="max-sm:sr-only"> days</span>
              </button>
            ))}
          </div>
          <form method="post" action="/logout">
            <DitherButton type="submit" color="grey" variant="dotted" className="font-sans text-[0.8125rem]">
              Lock
            </DitherButton>
          </form>
        </div>
      </header>

      {load.state === "ready" ? (
        <Dashboard stats={load.stats} span={span} at={at} setAt={setAt} />
      ) : load.state === "failed" ? (
        <div className="py-24">
          <h1 className="font-display text-2xl font-bold">The numbers didn't load.</h1>
          <p className="mt-2 text-quiet">The stats service didn't answer. It may be restarting.</p>
          <DitherButton type="button" color="mauve" variant="gradient" className="mt-6 font-sans" onClick={refresh}>
            Try again
          </DitherButton>
        </div>
      ) : (
        <div aria-busy="true" aria-label="Loading" className="py-10">
          <div className="h-9 w-80 max-w-full rounded-md bg-raised" />
          <div className="mt-10 h-56 rounded-md bg-raised" />
          <div className="mt-4 h-24 rounded-md bg-raised" />
        </div>
      )}
    </div>
  )
}

function Dashboard({ stats, span, at, setAt }: { stats: Stats; span: number; at: number | null; setAt: (i: number | null) => void }) {
  const data = useMemo(() => rows(stats, span), [stats, span])
  const today = data[data.length - 1]
  const lastWeek = data[data.length - 8]
  const shown = data[at ?? data.length - 1]
  const dayName = weekday.format(new Date(`${stats.today}T00:00:00Z`))
  const anything = Object.keys(stats.days).length > 0
  // no entrance: it loads into a reading, and the numbers don't perform
  const chart = { margins: MARGINS, markerIndex: at, onHoverChange: setAt, animate: false }

  return (
    <main>
      <div className="pt-6 pb-8">
        {anything ? (
          <h1 className="max-w-[40ch] font-display text-[1.75rem] leading-[1.2] font-bold tracking-[-0.02em] text-balance sm:text-[2.125rem]">
            {plural(today.ran, "install")} ran today. <span className="text-quiet">{against(today.ran, lastWeek?.ran ?? 0, dayName)}</span>
          </h1>
        ) : (
          <>
            <h1 className="font-display text-[1.75rem] leading-[1.2] font-bold tracking-[-0.02em] sm:text-[2.125rem]">Nothing has reported in yet.</h1>
            <p className="mt-3 max-w-[62ch] text-quiet">
              Each install sends one anonymous ping a day once it's on a version that has it. Visits to the Discord link and tries at this
              login show up here as well, on the day they happen.
            </p>
          </>
        )}
      </div>

      <Readout row={shown} live={at !== null} />

      <section aria-label="Day by day" className="mt-2">
        <Strip title="Installs that ran" tall>
          <AreaChart data={data} config={INSTALLS} stackType="stacked" className="h-full w-full" {...chart}>
            <Grid />
            <YAxis tickFormatter={whole} />
            <Area dataKey="headset" variant="gradient" />
            <Area dataKey="desktop" variant="hatched" />
            <Area dataKey="android" variant="dotted" />
          </AreaChart>
        </Strip>
        <Strip title="New installs">
          <BarChart data={data} config={FRESH} stackType="stacked" className="h-full w-full" {...chart}>
            <YAxis tickFormatter={whole} tickCount={2} />
            <Bar dataKey="newHeadset" variant="gradient" />
            <Bar dataKey="newDesktop" variant="hatched" />
            <Bar dataKey="newAndroid" variant="dotted" />
          </BarChart>
        </Strip>
        <Strip title="Recordings and clips">
          <BarChart data={data} config={MADE} stackType="stacked" className="h-full w-full" {...chart}>
            <YAxis tickFormatter={whole} tickCount={2} />
            <Bar dataKey="recordings" variant="gradient" />
            <Bar dataKey="clips" variant="dotted" />
          </BarChart>
        </Strip>
        <Strip title="Discord visits">
          <BarChart data={data} config={DISCORD} className="h-full w-full" {...chart}>
            <YAxis tickFormatter={whole} tickCount={2} />
            <Bar dataKey="discord" variant="gradient" />
          </BarChart>
        </Strip>
        <Strip title="Wrong logins here" last>
          <BarChart data={data} config={DOOR} stackType="stacked" className="h-full w-full" {...chart} margins={{ ...MARGINS, bottom: 22 }}>
            <YAxis tickFormatter={whole} tickCount={2} />
            <XAxis dataKey="label" maxTicks={7} />
            <Bar dataKey="wrong" variant="gradient" />
            <Bar dataKey="locked" variant="hatched" />
          </BarChart>
        </Strip>
      </section>

      <section aria-label="This week" className="mt-14 grid gap-x-12 gap-y-10 sm:grid-cols-2 lg:grid-cols-3">
        {APPS.map((app) => (
          <Ranked key={app} title={`${NAMES[app]} versions`} rows={ranked(stats, `version:${app}:`, 7)} colour={COLOURS[app]} />
        ))}
        <Ranked title="Desktop systems" rows={ranked(stats, "os:desktop:", 7)} colour="sky" />
        <Ranked title="Discord visits from" rows={ranked(stats, "discord:from=", 30)} colour="purple" note="30 days" />
        <Ranked title="Discord visits by country" rows={ranked(stats, "discord:country=", 30)} colour="purple" note="30 days" />
      </section>

      <Settings stats={stats} />

      <footer className="mt-16">
        <p className="text-[0.8125rem] text-quiet">Every number is a total for a day. No install, device or address is stored, so none can be shown.</p>
        <div className="relative mt-6 h-16 overflow-hidden rounded-lg" aria-hidden>
          <DitherGradient from="mauve" direction="up" opacity={0.6} />
        </div>
      </footer>
    </main>
  )
}

const INSTALLS = Object.fromEntries(APPS.map((app) => [app, { label: NAMES[app], color: COLOURS[app] }]))
const FRESH = {
  newHeadset: { label: "Headset", color: "mauve" },
  newDesktop: { label: "Desktop", color: "sky" },
  newAndroid: { label: "Android", color: "mint" },
} as const
const MADE = { recordings: { label: "Recordings", color: "peach" }, clips: { label: "Clips", color: "pink" } } as const
const DISCORD = { discord: { label: "Visits", color: "purple" } } as const
const DOOR = { wrong: { label: "Wrong", color: "red" }, locked: { label: "Locked out", color: "orange" } } as const

/** One chart of the timeline: its name beside it on a wide screen, over it on a narrow one. */
function Strip({ title, tall, last, children }: { title: string; tall?: boolean; last?: boolean; children: ReactNode }) {
  return (
    <div className="grid grid-cols-1 gap-x-6 border-t border-line py-3 sm:grid-cols-[9.5rem_1fr]">
      <h2 className="pb-1 text-[0.8125rem] font-medium text-quiet sm:pt-1.5">{title}</h2>
      {/* min-w-0: a grid item won't shrink below its canvas otherwise, and the chart runs off a phone */}
      <div className={`min-w-0 ${tall ? "h-48 sm:h-60" : last ? "h-28" : "h-24"}`}>{children}</div>
    </div>
  )
}

/** The day under the pointer, or today: every chart's value for it in one line. It doubles as the key. */
function Readout({ row, live }: { row: Row; live: boolean }) {
  const cell = (name: string, n: number, colour: string, texture: "solid" | "hatched" | "dotted") => (
    <div className="flex items-baseline gap-2">
      <span className={`swatch swatch-${texture} translate-y-px ${INK[colour]}`} aria-hidden />
      <dt className="text-[0.8125rem] text-quiet">{name}</dt>
      <dd className="num text-[0.9375rem] font-medium">{n.toLocaleString("en")}</dd>
    </div>
  )
  return (
    <dl aria-live="polite" className="sticky top-0 z-10 -mx-4 flex flex-wrap items-baseline gap-x-6 gap-y-1.5 bg-background/95 px-4 py-3 sm:-mx-8 sm:px-8">
      <div className="mr-2 min-w-[4.5rem]">
        <dt className="sr-only">Day</dt>
        <dd className={`num text-[0.9375rem] font-medium ${live ? "text-mauve" : ""}`}>{live ? row.label : "Today"}</dd>
      </div>
      {APPS.map((app) => cell(NAMES[app], row[app], COLOURS[app], TEXTURES[app]))}
      {cell("New", row.fresh, "quiet", "solid")}
      {cell("Recordings", row.recordings, "peach", "solid")}
      {cell("Clips", row.clips, "pink", "dotted")}
      {cell("Discord", row.discord, "purple", "solid")}
      {row.wrong + row.locked > 0 && cell("Wrong logins", row.wrong + row.locked, "red", "solid")}
    </dl>
  )
}

/** A list of things and how many, biggest first, each with its share as a dithered bar. */
function Ranked({ title, rows, colour, note = "this week" }: { title: string; rows: [string, number][]; colour: DitherColor; note?: string }) {
  const total = rows.reduce((n, [, count]) => n + count, 0)
  return (
    <div>
      <h2 className="flex items-baseline justify-between border-b border-line pb-2 text-[0.9375rem] font-medium">
        {title} <span className="text-xs font-normal text-faint">{note}</span>
      </h2>
      {rows.length === 0 ? (
        <p className="pt-3 text-[0.8125rem] text-faint">Nothing yet.</p>
      ) : (
        <ol>
          {rows.slice(0, 8).map(([name, count]) => (
            <li key={name} className="grid grid-cols-[1fr_auto] items-center gap-x-3 pt-2.5">
              <span className="truncate">{name}</span>
              <span className="num text-quiet">
                {count.toLocaleString("en")} <span className="text-faint">· {Math.round((count / total) * 100)}%</span>
              </span>
              <span className="col-span-2 mt-1 block h-1.5 rounded-[2px] bg-raised">
                <span className="relative block h-full overflow-hidden rounded-[2px]" style={{ width: `${Math.max((count / total) * 100, 2)}%` }}>
                  <DitherGradient from={colour} direction="right" cell={2} />
                </span>
              </span>
            </li>
          ))}
        </ol>
      )}
    </div>
  )
}

const SETTING_NAMES: Record<string, string> = {
  shape: "Shape",
  eye: "Eye",
  quality: "Quality",
  fps: "Frame rate",
  codec: "Codec",
  format: "File format",
  clips: "Clips",
  mic: "Microphone",
  split: "Split tracks",
  panels: "Panels unlocked",
}

/** How the headset's settings stand this week: each setting's values and the share of headsets on it. */
function Settings({ stats }: { stats: Stats }) {
  const byKey = new Map<string, [string, number][]>()
  for (const [pair, n] of ranked(stats, "setting:", 7)) {
    const [key, value] = pair.split("=")
    byKey.set(key, [...(byKey.get(key) ?? []), [value, n]])
  }
  if (byKey.size === 0) return null
  return (
    <section className="mt-14">
      <h2 className="flex items-baseline justify-between border-b border-line pb-2 text-[0.9375rem] font-medium">
        How headsets are set up <span className="text-xs font-normal text-faint">this week</span>
      </h2>
      <dl className="grid gap-x-12 sm:grid-cols-2">
        {Object.keys(SETTING_NAMES)
          .filter((key) => byKey.has(key))
          .map((key) => {
            const values = byKey.get(key)!
            const total = values.reduce((n, [, count]) => n + count, 0)
            return (
              <div key={key} className="flex items-baseline justify-between gap-4 border-b border-line/60 py-2.5">
                <dt className="text-quiet">{SETTING_NAMES[key]}</dt>
                <dd className="flex flex-wrap justify-end gap-x-4">
                  {values.map(([value, count]) => (
                    <span key={value}>
                      {value} <span className="num text-faint">{Math.round((count / total) * 100)}%</span>
                    </span>
                  ))}
                </dd>
              </div>
            )
          })}
      </dl>
    </section>
  )
}
