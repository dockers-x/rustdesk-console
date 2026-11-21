const platforms = [
  [/\bAndroid\b/i, "Android"],
  [/\b(?:iPhone|iPad|iPod|iOS)\b/i, "iOS"],
  [/\bWindows\b/i, "Windows"],
  [/\bCrOS\b/i, "ChromeOS"],
  [/\b(?:Macintosh|Mac OS X|macOS)\b/i, "macOS"],
  [/\bLinux\b/i, "Linux"],
] as const;

const browsers = [
  [/\bElectron\//i, "Electron"],
  [/\bEdg(?:A|iOS|e)?\//i, "Edge"],
  [/\b(?:OPR|Opera)\//i, "Opera"],
  [/\bSamsungBrowser\//i, "Samsung Internet"],
  [/\b(?:Firefox|FxiOS)\//i, "Firefox"],
  [/\b(?:Chrome|Chromium|CriOS)\//i, "Chrome"],
  [/\bVersion\/[\d.]+.*\bSafari\//i, "Safari"],
] as const;

export function summarizeLoginPlatform(value: string) {
  const raw = value.trim();
  if (!raw) return { summary: "", raw };
  const platform = platforms.find(([pattern]) => pattern.test(raw))?.[1];
  const userAgent = /\b(?:Mozilla|AppleWebKit|Chrome|Firefox|Electron)\//i.test(raw);
  if (!userAgent) return { summary: raw.length <= 38 && platform ? platform : raw, raw };

  const browser = browsers.find(([pattern]) => pattern.test(raw))?.[1];
  return { summary: [platform, browser].filter(Boolean).join(" · "), raw };
}
