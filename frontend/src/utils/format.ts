export function formatCode(code: string) {
  return code.length === 10 ? code.replace(/(.{5})(.{5})/, "$1-$2") : code;
}

/** unixSeconds is a Unix timestamp in SECONDS, per the backend's GuestPass shape. */
export function formatDateTime(unixSeconds: number | null | undefined) {
  if (unixSeconds == null) return "—";
  return new Date(unixSeconds * 1000).toLocaleString();
}

export function formatStatus(used: boolean) {
  return used ? "Used" : "Available";
}

/** durationSecs is validTimeSecs, granted once the guest first uses the pass. */
export function formatDurationSecs(durationSecs: number | null | undefined) {
  if (!durationSecs) return "0m";
  const days = Math.floor(durationSecs / 86400),
    hours = Math.floor((durationSecs % 86400) / 3600),
    mins = Math.floor((durationSecs % 3600) / 60);
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

/** shareNumber === 0 means unlimited devices, per verified controller behaviour. */
export function formatDevices(shareNumber: number) {
  return shareNumber === 0 ? "Unlimited" : String(shareNumber);
}
