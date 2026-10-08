export function formatCode(code: string) {
  return code.length === 10 ? code.replace(/(.{5})(.{5})/, "$1-$2") : code;
}

export function formatMaxGuests(maxGuests: number | null | undefined) {
  return !maxGuests ? "Unlimited" : Math.max(maxGuests, 0);
}

export function formatStatus(
  expired: boolean,
  activatedAt: string | null | undefined,
) {
  if (expired) return "Expired";
  if (activatedAt) return "Active";
  return "Available";
}

export function formatDuration(m: number | null | undefined) {
  if (!m) return "Unlimited";
  const days = Math.floor(m / 1440),
    hours = Math.floor((m % 1440) / 60),
    mins = m % 60;
  return (
    [
      days > 0 ? days + "d" : "",
      hours > 0 ? hours + "h" : "",
      mins > 0 ? mins + "m" : "",
    ]
      .filter(Boolean)
      .join(" ") || "0m"
  );
}

export function formatGuestUsage(
  usage: number,
  limit: number | null | undefined,
) {
  return limit ? `${usage}/${limit}` : `${usage}/∞`;
}
