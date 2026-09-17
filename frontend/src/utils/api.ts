import { GuestPass, PassCreateData } from "@/types/voucher";
import { notifyVouchersUpdated } from "./actions";

async function call<T>(endpoint: string, opts: RequestInit = {}) {
  const res = await fetch(`/rust-api${endpoint}`, {
    headers: { "Content-Type": "application/json" },
    ...opts,
  });
  if (!res.ok) {
    const error = new Error(res.statusText);
    (error as any).status = res.status;
    throw error;
  }
  return res.json() as Promise<T>;
}

export const MIN_PASS_DURATION_HOURS = 1;
/** One year. The controller counts duration in hours only. */
export const MAX_PASS_DURATION_HOURS = 8760;

/** 0 is valid and means unlimited devices. */
export const MIN_PASS_SHARES = 0;
export const MAX_PASS_SHARES = 1000;

/**
 * Passes can NEVER be deleted -- the Guest Pass Manager account has no
 * delete capability -- and the controller rejects a duplicate name outright
 * with E_DuplicatedValue. A fixed generated name therefore works exactly
 * once and then fails forever, with no way to recover. Every name the UI
 * generates for the user carries a unique suffix so that cannot happen.
 *
 * This is for UI-generated names only. The daily rotation's `daily-<date>`
 * naming is deliberately deterministic: its duplicate rejection is the
 * idempotency mechanism that stops concurrent replicas double-minting.
 */
export function uniqueNameSuffix(): string {
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`;
}

/**
 * A 409 means the controller already has a pass with that name. Since
 * nothing can delete it, that is permanent for that name -- worth saying
 * explicitly rather than reporting a generic failure the user cannot act on.
 */
export function createPassErrorMessage(error: unknown): string {
  if ((error as { status?: number } | null)?.status === 409) {
    return "A pass with that name already exists — try a different name";
  }
  return "Failed to create guest pass";
}

export const api = {
  getAllPasses: () => call<GuestPass[]>("/passes"),

  getDailyPass: () => call<GuestPass>("/passes/daily"),

  createPass: async (data: PassCreateData) => {
    const result = await call<GuestPass>("/passes", {
      method: "POST",
      body: JSON.stringify(data),
    });
    await notifyVouchersUpdated();
    return result;
  },
};
