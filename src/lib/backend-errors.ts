// The backend prefixes a handful of auth/hardware errors with a stable,
// language-independent code (e.g. "DEVICE_LIMIT_REACHED::<translated
// message>") specifically so the UI can strip it for display and react to it
// (offer a way to fix it) without pattern-matching on prose that changes per
// locale. Errors without a known prefix pass through untouched.
const BACKEND_ERROR_CODES = [
  "HARDWARE_INACTIVE",
  "DEVICE_LIMIT_REACHED",
  "HARDWARE_ALREADY_LINKED",
  "DEVICE_TRANSFER_COOLDOWN",
] as const;

// No regex: the message half is free-form translated prose that can contain
// newlines and regex metacharacters, and building a pattern from these codes
// invites escaping bugs for no benefit over a plain prefix scan.
export function splitBackendErrorCode(raw: string): { code: string | null; message: string } {
  for (const code of BACKEND_ERROR_CODES) {
    const marker = code + "::";
    const at = raw.indexOf(marker);
    if (at !== -1) return { code, message: raw.slice(at + marker.length).trim() };
  }
  return { code: null, message: raw };
}
