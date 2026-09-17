"use client";

import SuccessModal from "@/components/modals/SuccessModal";
import { GuestPass } from "@/types/voucher";
import { api, createPassErrorMessage, uniqueNameSuffix } from "@/utils/api";
import { notify } from "@/utils/notifications";
import { useCallback, useState } from "react";

const DURATION_PRESETS_HOURS = [1, 4, 8, 24, 72, 168];

function formatPresetLabel(hours: number): string {
  if (hours % 24 === 0 && hours >= 24) {
    const days = hours / 24;
    return `${days} Day${days === 1 ? "" : "s"}`;
  }
  return `${hours} Hour${hours === 1 ? "" : "s"}`;
}

export default function QuickCreateTab() {
  const [loading, setLoading] = useState<number | null>(null);
  const [newPass, setNewPass] = useState<GuestPass | null>(null);

  const handleCreate = async (durationHours: number) => {
    setLoading(durationHours);
    try {
      const pass = await api.createPass({
        // The suffix is not decoration: without it every preset name is
        // fixed, the controller rejects the second click as a duplicate,
        // and the button is permanently one-shot -- passes cannot be
        // deleted to free the name up again.
        name: `Quick Pass (${formatPresetLabel(durationHours)}) ${uniqueNameSuffix()}`,
        durationHours,
        shareNumber: 0,
      });
      setNewPass(pass);
    } catch (error) {
      notify(createPassErrorMessage(error), "error");
    }
    setLoading(null);
  };

  const closeModal = useCallback(() => {
    setNewPass(null);
  }, []);

  return (
    <div>
      <div className="card max-w-lg mx-auto space-y-4">
        <p className="text-secondary text-sm">
          Create an unlimited-device guest pass for a preset duration.
        </p>
        <div className="grid grid-cols-2 gap-3">
          {DURATION_PRESETS_HOURS.map((hours) => (
            <button
              key={hours}
              type="button"
              onClick={() => handleCreate(hours)}
              disabled={loading !== null}
              className="btn-primary"
            >
              {loading === hours ? "Creating…" : formatPresetLabel(hours)}
            </button>
          ))}
        </div>
      </div>

      {newPass && <SuccessModal pass={newPass} onClose={closeModal} />}
    </div>
  );
}
