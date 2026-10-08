// The controller refuses these characters in a voucher name (it answers OK
// and creates nothing), and garbles anything outside printable ASCII. These
// mirror the backend's checks so staff see the problem before submitting.
export const NAME_REJECTED_CHARS = "'\"<>&#;\\`|!$()";
export const REMARKS_REJECTED_CHARS = "<>";

const PRINTABLE_ASCII = /^[\x20-\x7e]*$/;

/** A message describing what is wrong with `value`, or null if it is fine. */
export function textProblem(
  field: string,
  value: string,
  rejected: string,
): string | null {
  if (!PRINTABLE_ASCII.test(value)) {
    return `${field} can only contain plain ASCII characters`;
  }
  const bad = [...value].find((c) => rejected.includes(c));
  return bad ? `${field} cannot contain ${bad}` : null;
}
