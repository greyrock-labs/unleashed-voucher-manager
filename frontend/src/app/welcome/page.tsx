"use client";

import { useGlobal } from "@/contexts/GlobalContext";
import { api } from "@/utils/api";
import { useCallback, useEffect, useState } from "react";

export default function WelcomePage() {
  const [visited, setVisited] = useState(false);
  const { wifiConfig } = useGlobal();

  const rotateVoucher = useCallback(async () => {
    try {
      // Returns the waiting rolling voucher, creating one only if none waits
      await api.createRollingVoucher();
    } catch (error) {
      console.error("Failed to create rolling voucher", error);
    }
  }, []);

  useEffect(() => {
    if (visited) return;

    rotateVoucher();
    setVisited(true);
  }, [rotateVoucher, visited]);

  return (
    <main className="flex-center h-screen w-full px-4">
      <div className="w-full text-center font-bold text-4xl sm:text-5xl md:text-7xl lg:text-9xl leading-snug">
        {wifiConfig?.ssid ? (
          <>
            Welcome to{" "}
            <span className="text-brand font-mono">{wifiConfig.ssid}</span>!
          </>
        ) : (
          "Welcome!"
        )}
      </div>
    </main>
  );
}
