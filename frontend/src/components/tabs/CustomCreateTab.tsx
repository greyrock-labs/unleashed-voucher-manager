"use client";

import SuccessModal from "@/components/modals/SuccessModal";
import { GuestPass, PassCreateData } from "@/types/voucher";
import {
  api,
  createPassErrorMessage,
  MAX_PASS_DURATION_HOURS,
  MAX_PASS_SHARES,
  MIN_PASS_DURATION_HOURS,
  MIN_PASS_SHARES,
  uniqueNameSuffix,
} from "@/utils/api";
import { notify } from "@/utils/notifications";
import { useCallback, useMemo, useState, SubmitEvent } from "react";

/**
 * The default name has to differ every time. Passes cannot be deleted, and
 * the controller rejects a duplicate name, so a fixed default made the form
 * usable exactly once unless the user thought to retype the name.
 */
function defaultPassName(): string {
  return `Custom Guest Pass ${uniqueNameSuffix()}`;
}

export default function CustomCreateTab() {
  const [loading, setLoading] = useState(false);
  const [newPass, setNewPass] = useState<GuestPass | null>(null);
  // Bumped after a successful create to remount the form, which clears the
  // fields and re-seeds the name with a fresh unique default. `form.reset()`
  // would restore the *old* default name instead.
  const [formKey, setFormKey] = useState(0);
  // Pinned to formKey so unrelated re-renders (the loading flag, the success
  // modal) cannot swap the name out from under a pristine field mid-submit.
  const defaultName = useMemo(() => defaultPassName(), [formKey]);

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
      setFormKey((key) => key + 1);
    } catch (error) {
      notify(createPassErrorMessage(error), "error");
    }
    setLoading(false);
  };

  const closeModal = useCallback(() => {
    setNewPass(null);
  }, []);

  return (
    <div>
      <form
        key={formKey}
        onSubmit={handleSubmit}
        className="card max-w-lg mx-auto space-y-6"
      >
        <div>
          <label className="block font-medium mb-1">Name</label>
          <input
            name="name"
            type="text"
            required
            defaultValue={defaultName}
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
