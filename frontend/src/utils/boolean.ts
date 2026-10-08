/**
 * Read a boolean setting. Environment variables reach the runtime config as
 * strings, and "false" is truthy, so strings are parsed rather than trusted.
 * Anything unreadable falls back to `fallback`.
 */
export function parseBoolean(value: unknown, fallback: boolean): boolean {
  if (typeof value === "boolean") return value;
  if (typeof value !== "string") return fallback;
  switch (value.trim().toLowerCase()) {
    case "true":
    case "yes":
    case "1":
      return true;
    case "false":
    case "no":
    case "0":
      return false;
    default:
      return fallback;
  }
}
