"use server";

import { sseManager } from "./sseManager";

export async function notifyVouchersUpdated() {
  sseManager.broadcastToClients({
    type: "vouchersUpdated",
    timestamp: Date.now(),
  });
}
