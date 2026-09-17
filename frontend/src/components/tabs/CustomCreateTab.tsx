"use client";

import SuccessModal from "@/components/modals/SuccessModal";
import { GuestPass, PassCreateData } from "@/types/voucher";
import {
  api,
  MAX_PASS_DURATION_HOURS,
  MAX_PASS_SHARES,
  MIN_PASS_DURATION_HOURS,
  MIN_PASS_SHARES,
} from "@/utils/api";
import { notify } from "@/utils/notifications";
import { useCallback, useState, SubmitEvent } from "react";

export default function CustomCreateTab() {
  const [loading, setLoading] = useState(false);
  const [newPass, setNewPass] = useState<GuestPass | null>(null);

  const handleSubmit = async (e: SubmitEvent) => {
    e.preventDefault();
    setLoading(true);

    const form = e.currentTarget as HTMLFormElement;
    const data = new FormData(form);

    const payload: PassCreateData = {
      name: String(data.get("name")),
      durationHours: Number(data.get("duration")),
      shareNumber: Number(data.get("shareNumber")),
    };

    try {
      const pass = await api.createPass(payload);
      setNewPass(pass);
      notify("Successfully created guest pass", "success");
      form.reset();
    } catch {
      notify("Failed to create guest pass", "error");
    }
    setLoading(false);
  };

  const closeModal = useCallback(() => {
    setNewPass(null);
  }, []);

  return (
    <div>
      <form onSubmit={handleSubmit} className="card max-w-lg mx-auto space-y-6">
        <div>
          <label className="block font-medium mb-1">Name</label>
          <input
            name="name"
            type="text"
            required
            defaultValue="Custom Guest Pass"
          />
        </div>

        <div>
          <label className="block font-medium mb-1">Duration (hours)</label>
          <input
            name="duration"
            type="number"
            required
            min={MIN_PASS_DURATION_HOURS}
            max={MAX_PASS_DURATION_HOURS}
            defaultValue={24}
          />
        </div>

        <div>
          <label className="block font-medium mb-1">
            Devices (0 = unlimited)
          </label>
          <input
            name="shareNumber"
            type="number"
            required
            min={MIN_PASS_SHARES}
            max={MAX_PASS_SHARES}
            defaultValue={0}
          />
        </div>

        <button type="submit" disabled={loading} className="btn-primary w-full">
          {loading ? "Creating…" : "Create Guest Pass"}
        </button>
      </form>
      {newPass && <SuccessModal pass={newPass} onClose={closeModal} />}
    </div>
  );
}
