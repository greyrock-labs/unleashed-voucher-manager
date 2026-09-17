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
      <main className="flex-center h-screen h-dvh bg-page">
        <Spinner />
      </main>
    );
  }

  return (
    // h-dvh, not min-h-dvh: a definite height is what lets the QR below give
    // way instead of the page growing. With min-h- the content set the height
    // and justify-center then bled the overflow off both ends, which is how
    // the "Scan to join" caption ended up under the fold.
    <main className="flex h-screen h-dvh flex-col items-center justify-center gap-[clamp(0.5rem,2.5vh,2rem)] bg-page p-4 text-center sm:p-6">
      <h1 className="font-light leading-tight text-secondary text-[clamp(1.125rem,min(4vw,4vh),1.875rem)]">
        Guest WiFi
      </h1>

      {status === "ok" && pass ? (
        <>
          {/* Sized against the viewport rather than width breakpoints. This
              screen is whatever a wall-mounted tablet or TV happens to be, and
              the binding constraint is its height, which width breakpoints
              know nothing about -- sm: fires the same on a 1024x768 panel as
              on a 1024x400 one. min() also keeps the 11 monospace characters
              inside a narrow viewport, and the clamp bounds keep it legible on
              a phone without ballooning on a 4K panel. */}
          <p className="voucher-code leading-none text-[clamp(2.5rem,min(12vw,12vh),8rem)]">
            {formatCode(pass.code)}
          </p>

          {/* flex-1 min-h-0 hands the QR whatever height is left, so the
              caption cannot be pushed off-screen: the box shrinks instead.

              An explicit box is still required. WifiQr measures its container
              and sizes the QR to a fraction of it; as a content-sized flex
              item it had no intrinsic size, so each measurement shrank the box
              and re-fired the ResizeObserver, converging down to the caption
              width or the 32px floor. A flex-1 height resolves from the
              parent's free space rather than from content, so that loop cannot
              start.

              sizeRatio is below the 0.8 default to leave the last quarter of
              the box for the caption and its gap, which at 0.8 would be
              squeezed past the bottom edge of the box itself. */}
          <WifiQr
            className="min-h-0 w-full flex-1"
            sizeRatio={0.75}
            imageSrc="/logo-mark.png"
          />
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
