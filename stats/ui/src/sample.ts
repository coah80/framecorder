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
      "count:minutes": Math.round(h * rand(9, 20)),
      "count:live": Math.round(h * rand(0.1, 0.4)),
      "count:remote": Math.round(h * rand(0.05, 0.2)),
      "count:synced": Math.round((d + a) * rand(2, 5)),
      "count:synced_mb": Math.round((d + a) * rand(300, 900)),
      "count:reports": rand(0, 1) > 0.8 ? 1 : 0,
      "country:US": Math.round((h + d + a) * 0.44),
      "country:DE": Math.round((h + d + a) * 0.14),
      "country:GB": Math.round((h + d + a) * 0.11),
      "country:JP": Math.round((h + d + a) * 0.08),
      "country:FR": Math.round((h + d + a) * 0.07),
      "country:CA": Math.round((h + d + a) * 0.06),
      "age:headset:0": Math.round(h * 0.07),
      "age:headset:1-6": Math.round(h * 0.2),
      "age:headset:7-29": Math.round(h * 0.38),
      "age:headset:30-89": Math.round(h * 0.25),
      "age:headset:90+": Math.round(h * 0.1),
      "age:desktop:7-29": Math.round(d * 0.6),
      "age:desktop:30-89": Math.round(d * 0.4),
      "age:android:1-6": Math.round(a * 0.3),
      "age:android:7-29": Math.round(a * 0.7),
      ...Object.fromEntries(Array.from({ length: 24 }, (_, hour) => [`hour:${String(hour).padStart(2, "0")}`, Math.round((h + d + a) * (0.02 + 0.04 * Math.max(0, Math.sin(((hour - 13) / 24) * Math.PI * 2))))])),
      // 0.2.0 takes over from 0.1.4 across the last three weeks
      "version:headset:0.2.0": Math.round(h * Math.min(0.95, Math.max(0, (21 - i) / 14))),
      "version:headset:0.1.4": h - Math.round(h * Math.min(0.95, Math.max(0, (21 - i) / 14))),
      "version:desktop:0.2.0": d,
      "version:android:0.2.0": a,
      "os:desktop:windows": Math.round(d * 0.62),
      "os:desktop:linux": Math.round(d * 0.24),
      "os:desktop:macos": d - Math.round(d * 0.62) - Math.round(d * 0.24),
      "setting:headset:shape=wide": Math.round(h * 0.7),
      "setting:headset:shape=tall": Math.round(h * 0.15),
      "setting:headset:shape=square": Math.round(h * 0.1),
      "setting:headset:shape=both": Math.round(h * 0.05),
      "setting:headset:codec=hevc": Math.round(h * 0.85),
      "setting:headset:codec=h264": h - Math.round(h * 0.85),
      "setting:headset:clips=on": Math.round(h * 0.9),
      "setting:headset:clips=off": h - Math.round(h * 0.9),
      "setting:headset:mic=on": Math.round(h * 0.75),
      "setting:headset:mic=off": h - Math.round(h * 0.75),
      "setting:headset:paired=0": Math.round(h * 0.2),
      "setting:headset:paired=1": Math.round(h * 0.5),
      "setting:headset:paired=2": Math.round(h * 0.22),
      "setting:headset:paired=3+": Math.round(h * 0.08),
      "setting:headset:folder=videos": Math.round(h * 0.82),
      "setting:headset:folder=drive": Math.round(h * 0.18),
      "setting:desktop:view=grid": Math.round(d * 0.78),
      "setting:desktop:view=list": Math.round(d * 0.22),
      "setting:desktop:background=on": Math.round(d * 0.9),
      "setting:desktop:background=off": Math.round(d * 0.1),
      "setting:desktop:arch=x86_64": Math.round(d * 0.88),
      "setting:desktop:arch=aarch64": Math.round(d * 0.12),
      "setting:android:sdk=36": Math.round(a * 0.55),
      "setting:android:sdk=35": Math.round(a * 0.3),
      "setting:android:sdk=34": Math.round(a * 0.15),
      "setting:android:background=on": Math.round(a * 0.7),
      "setting:android:background=off": Math.round(a * 0.3),
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
