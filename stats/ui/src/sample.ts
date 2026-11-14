// made-up numbers for working on the page (`npm run dev`). the build never
// includes this file: app.tsx only imports it when vite is serving in dev
import type { Stats } from "./data"

export function sample(): Stats {
  let seed = 7
  const rand = (a: number, b: number) => {
    seed = (seed * 1103515245 + 12345) % 2147483648
    return a + (seed / 2147483648) * (b - a)
  }
  const today = new Date().toISOString().slice(0, 10)
  const end = Date.parse(`${today}T00:00:00Z`)
  const days: Stats["days"] = {}
  for (let i = 89; i >= 0; i--) {
    const day = new Date(end - i * 86_400_000).toISOString().slice(0, 10)
    const grow = 1 + (89 - i) / 30
    const [h, d, a] = [rand(50, 70) * grow, rand(30, 42) * grow, rand(20, 30) * grow].map(Math.round)
    days[day] = {
      "active:headset": h,
      "active:desktop": d,
      "active:android": a,
      "new:headset": Math.round(h * rand(0.04, 0.12)),
      "new:desktop": Math.round(d * rand(0.03, 0.1)),
      "new:android": Math.round(a * rand(0.04, 0.12)),
      "count:recordings": Math.round(h * rand(1.2, 2.2)),
      "count:clips": Math.round(h * rand(3, 6)),
      "version:headset:0.2.0": Math.round(h * 0.8),
      "version:headset:0.1.4": h - Math.round(h * 0.8),
      "version:desktop:0.2.0": d,
      "version:android:0.2.0": a,
      "os:desktop:windows": Math.round(d * 0.62),
      "os:desktop:linux": Math.round(d * 0.24),
      "os:desktop:macos": d - Math.round(d * 0.62) - Math.round(d * 0.24),
      "setting:shape=wide": Math.round(h * 0.7),
      "setting:shape=tall": Math.round(h * 0.15),
      "setting:shape=square": Math.round(h * 0.1),
      "setting:shape=both": Math.round(h * 0.05),
      "setting:codec=hevc": Math.round(h * 0.85),
      "setting:codec=h264": h - Math.round(h * 0.85),
      "setting:clips=on": Math.round(h * 0.9),
      "setting:clips=off": h - Math.round(h * 0.9),
      "setting:mic=on": Math.round(h * 0.75),
      "setting:mic=off": h - Math.round(h * 0.75),
      "discord:from=desktop": Math.round(rand(1, 6)),
      "discord:from=android": Math.round(rand(0, 4)),
      "discord:from=site": Math.round(rand(0, 3)),
      "discord:country=US": Math.round(rand(2, 7)),
      "discord:country=DE": Math.round(rand(0, 3)),
      "discord:country=GB": Math.round(rand(0, 2)),
      "auth:wrong": rand(0, 1) > 0.85 ? Math.round(rand(1, 6)) : 0,
      "auth:in": rand(0, 1) > 0.6 ? 1 : 0,
    }
  }
  return { today, days }
}
