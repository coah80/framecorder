// sizes, lengths and dates the way people say them.

export function size(bytes) {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let n = bytes, i = 0;
  while (n >= 1000 && i < units.length - 1) { n /= 1000; i++; }
  return `${n.toFixed(n < 10 && i > 0 ? 1 : 0)} ${units[i]}`;
}

export function length(secs) {
  if (secs == null) return "";
  const s = Math.round(secs);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), r = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${r}` : `${m}:${r}`;
}

export function time(unix) {
  return new Date(unix * 1000).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" }).toLowerCase();
}

const startOfDay = (d) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();

// "today", "yesterday", then "sun 28 sep"
export function day(unix) {
  const d = new Date(unix * 1000);
  const days = Math.round((startOfDay(new Date()) - startOfDay(d)) / 86_400_000);
  if (days === 0) return "today";
  if (days === 1) return "yesterday";
  return d.toLocaleDateString(undefined, { weekday: "short", day: "numeric", month: "short" }).toLowerCase();
}

export const dayKey = (unix) => startOfDay(new Date(unix * 1000));
