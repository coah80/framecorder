// what /api/stats sends, and the shapes the page draws from it

export type Counts = Record<string, number>
export type Stats = { today: string; days: Record<string, Counts> }

export const APPS = ["headset", "desktop", "android"] as const
export type App = (typeof APPS)[number]

export type Row = {
  day: string
  /** "Nov 9" */
  label: string
  headset: number
  desktop: number
  android: number
  ran: number
  newHeadset: number
  newDesktop: number
  newAndroid: number
  fresh: number
  recordings: number
  clips: number
  discord: number
  wrong: number
  locked: number
}

const DAY = 86_400_000

const month = new Intl.DateTimeFormat("en", { month: "short", day: "numeric", timeZone: "UTC" })
export const weekday = new Intl.DateTimeFormat("en", { weekday: "long", timeZone: "UTC" })

const sum = (counts: Counts, prefix: string) =>
  Object.entries(counts).reduce((n, [key, value]) => (key.startsWith(prefix) ? n + value : n), 0)

/** One row per day for the `span` days ending today, oldest first; a day nothing reported is zeros. */
export function rows(stats: Stats, span: number): Row[] {
  const end = Date.parse(`${stats.today}T00:00:00Z`)
  return Array.from({ length: span }, (_, i) => {
    const date = new Date(end - (span - 1 - i) * DAY)
    const day = date.toISOString().slice(0, 10)
    const c = stats.days[day] ?? {}
    const [headset, desktop, android] = APPS.map((app) => c[`active:${app}`] ?? 0)
    const [newHeadset, newDesktop, newAndroid] = APPS.map((app) => c[`new:${app}`] ?? 0)
    return {
      day,
      label: month.format(date),
      headset,
      desktop,
      android,
      ran: headset + desktop + android,
      newHeadset,
      newDesktop,
      newAndroid,
      fresh: newHeadset + newDesktop + newAndroid,
      recordings: c["count:recordings"] ?? 0,
      clips: c["count:clips"] ?? 0,
      discord: sum(c, "discord:from="),
      wrong: c["auth:wrong"] ?? 0,
      locked: c["auth:locked"] ?? 0,
    }
  })
}

/** Counters starting with `prefix`, added up over the last `span` days, biggest first. */
export function ranked(stats: Stats, prefix: string, span: number): [string, number][] {
  const end = Date.parse(`${stats.today}T00:00:00Z`)
  const totals = new Map<string, number>()
  for (let i = 0; i < span; i++) {
    const c = stats.days[new Date(end - i * DAY).toISOString().slice(0, 10)] ?? {}
    for (const [key, n] of Object.entries(c)) {
      if (key.startsWith(prefix)) totals.set(key.slice(prefix.length), (totals.get(key.slice(prefix.length)) ?? 0) + n)
    }
  }
  return [...totals].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
}

/** How today compares with the same weekday last week, as a sentence's second half. */
export function against(today: number, lastWeek: number, dayName: string): string {
  if (lastWeek === 0) return today === 0 ? "" : `None ran last ${dayName}.`
  const change = Math.round(((today - lastWeek) / lastWeek) * 100)
  if (change === 0) return `The same as last ${dayName}.`
  return `${Math.abs(change)}% ${change > 0 ? "more" : "fewer"} than last ${dayName}.`
}

export const plural = (n: number, one: string, many = `${one}s`) => `${n.toLocaleString("en")} ${n === 1 ? one : many}`
