import { GuestPass, PassCreateData } from "@/types/voucher";
import { notifyVouchersUpdated } from "./actions";

async function call<T>(endpoint: string, opts: RequestInit = {}) {
  const res = await fetch(`/rust-api/${endpoint}`, {
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
