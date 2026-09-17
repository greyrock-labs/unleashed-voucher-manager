"use client";

import { useCallback, useEffect, useState } from "react";
import { api } from "@/utils/api";
import { GuestPass } from "@/types/voucher";
import { useServerEvents } from "@/hooks/useServerEvents";
import WifiQr from "@/components/utils/WifiQr";
import Spinner from "@/components/utils/Spinner";
import { formatCode } from "@/utils/format";

/**
 * The three ways this screen can end up with nothing to show a guest:
 * - "loading": first fetch hasn't resolved yet.
 * - "no-pass": the backend answered but there is no pass for today (404).
 * - "unreachable": the backend didn't answer at all (network error, 5xx,
 *   proxy down, etc). Distinct from "no-pass" so a wall-mounted tablet
 *   doesn't quietly tell guests "no wifi today" when the real problem is
 *   that the controller or backend is down.
 */
type Status = "loading" | "ok" | "no-pass" | "unreachable";

export default function DisplayPage() {
  const [pass, setPass] = useState<GuestPass | null>(null);
  const [status, setStatus] = useState<Status>("loading");

  const refresh = useCallback(async () => {
    try {
      const dailyPass = await api.getDailyPass();
      setPass(dailyPass);
      setStatus("ok");
    } catch (err) {
      setPass(null);
      setStatus((err as { status?: number })?.status === 404 ? "no-pass" : "unreachable");
    }
  }, []);

  useEffect(() => {
    refresh();
    // The pass only changes once a day; poll hourly as a safety net so a
    // wall-mounted tablet recovers on its own even if it misses an SSE
    // update (missed event, brief backend restart, etc).
    const id = setInterval(refresh, 60 * 60 * 1000);
    return () => clearInterval(id);
  }, [refresh]);

  // Takes no arguments -- it dispatches a `vouchersUpdated` CustomEvent on
  // window whenever the backend pushes an update over SSE, which is what we
  // subscribe to below.
  useServerEvents();
  useEffect(() => {
    window.addEventListener("vouchersUpdated", refresh);
    return () => window.removeEventListener("vouchersUpdated", refresh);
  }, [refresh]);

  if (status === "loading") {
    return (
      <main className="flex-center min-h-screen min-h-dvh bg-page">
        <Spinner />
      </main>
    );
  }

  return (
    <main className="flex min-h-screen min-h-dvh flex-col items-center justify-center gap-6 bg-page p-6 text-center sm:gap-8 sm:p-8">
      <h1 className="text-2xl font-light text-secondary sm:text-3xl">
        Guest WiFi
      </h1>

      {status === "ok" && pass ? (
        <>
          <p className="voucher-code text-6xl sm:text-8xl lg:text-9xl">
            {formatCode(pass.code)}
          </p>

          {/* An explicit box is required. WifiQr measures its container and
              sizes the QR to a fraction of it; as a content-sized flex item
              it has no intrinsic size, so each measurement shrank the box
              and re-fired the ResizeObserver, converging down to the caption
              width or the 32px floor. Sized here for a wall-mounted tablet
              or TV. */}
          <WifiQr className="h-64 w-64 sm:h-96 sm:w-96" imageSrc="/logo-mark.png" />
        </>
      ) : (
        <p className="max-w-md text-xl text-muted sm:text-2xl">
          {status === "no-pass"
            ? "No guest pass is available right now. Please check back later."
            : "Guest WiFi status is temporarily unavailable."}
        </p>
      )}
    </main>
  );
}
