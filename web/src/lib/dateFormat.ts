function displayText(value: unknown) {
  if (value === null || value === undefined) return "";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

export function formatDateTime(value: unknown, timeZone?: string) {
  if (value instanceof Date) return formatDate(value, timeZone);
  if (typeof value !== "string" || value.trim() === "") {
    return displayText(value);
  }

  const trimmed = value.trim();
  const iso = trimmed.includes("T") ? trimmed : `${trimmed.replace(" ", "T")}Z`;
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return value;
  return formatDate(date, timeZone);
}

export function formatUnixSeconds(value: unknown, timeZone?: string) {
  const seconds =
    typeof value === "number"
      ? value
      : typeof value === "string"
        ? Number(value)
        : Number.NaN;
  if (!Number.isFinite(seconds) || seconds <= 0) return displayText(value);
  return formatDate(new Date(seconds * 1000), timeZone);
}

function formatDate(date: Date, timeZone?: string) {
  const options: Intl.DateTimeFormatOptions = {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
    hourCycle: "h23",
    ...(timeZone ? { timeZone } : {}),
  };

  try {
    return formatDateParts(date, options);
  } catch {
    const { timeZone: _timeZone, ...fallback } = options;
    return formatDateParts(date, fallback);
  }
}

function formatDateParts(date: Date, options: Intl.DateTimeFormatOptions) {
  const parts = new Intl.DateTimeFormat("en-US", options).formatToParts(date);
  const get = (type: Intl.DateTimeFormatPartTypes) =>
    parts.find((part) => part.type === type)?.value ?? "";
  return `${get("year")}-${get("month")}-${get("day")} ${get("hour")}:${get("minute")}:${get("second")}`;
}
