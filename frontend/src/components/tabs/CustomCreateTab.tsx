"use client";

import SuccessModal from "@/components/modals/SuccessModal";
import { Voucher, VoucherCreateData } from "@/types/voucher";
import {
  api,
  MAX_VOUCHER_COUNT,
  MAX_VOUCHER_DURATION_HOURS,
  MAX_VOUCHER_GUESTS,
  MAX_VOUCHER_KEY_LENGTH,
  MIN_VOUCHER_COUNT,
  MIN_VOUCHER_GUESTS,
  MIN_VOUCHER_KEY_LENGTH,
} from "@/utils/api";
import { map } from "@/utils/functional";
import { notify } from "@/utils/notifications";
import {
  NAME_REJECTED_CHARS,
  REMARKS_REJECTED_CHARS,
  textProblem,
} from "@/utils/validation";
import { useCallback, useState, SubmitEvent } from "react";

// The controller only accepts whole hours, days or weeks.
type TimeUnit = "hours" | "days" | "weeks";

const HOURS_PER_UNIT: Record<TimeUnit, number> = {
  hours: 1,
  days: 24,
  weeks: 168,
};

export default function CustomCreateTab() {
  const [loading, setLoading] = useState(false);
  const [newVouchers, setNewVouchers] = useState<Voucher[] | null>(null);
  const [durationUnit, setDurationUnit] = useState<TimeUnit>("hours");
  const [count, setCount] = useState(MIN_VOUCHER_COUNT);

  const handleSubmit = async (e: SubmitEvent) => {
    e.preventDefault();
    setLoading(true);

    const parseNumber = (x: FormDataEntryValue) =>
      x !== "" ? Number(x) : null;
    const parseText = (x: FormDataEntryValue) =>
      String(x).trim() !== "" ? String(x).trim() : null;

    const form = e.currentTarget as HTMLFormElement;
    const data = new FormData(form);

    const rawDuration = Number(data.get("duration"));
    const unit = String(data.get("durationUnit") || "hours") as TimeUnit;

    if (!Number.isInteger(rawDuration) || rawDuration <= 0) {
      notify("Duration must be a whole number above zero", "error");
      setLoading(false);
      return;
    }

    const durationHours = rawDuration * HOURS_PER_UNIT[unit];
    if (durationHours > MAX_VOUCHER_DURATION_HOURS) {
      notify(
        `Duration too long. Maximum allowed is ${MAX_VOUCHER_DURATION_HOURS} hours`,
        "error",
      );
      setLoading(false);
      return;
    }

    const name = String(data.get("name") ?? "");
    const remarks = String(data.get("remarks") ?? "");
    const problem =
      (Number(data.get("count")) === 1 &&
        textProblem("Name", name, NAME_REJECTED_CHARS)) ||
      textProblem("Remarks", remarks, REMARKS_REJECTED_CHARS);
    if (problem) {
      notify(problem, "error");
      setLoading(false);
      return;
    }

    const payload: VoucherCreateData = {
      count: Number(data.get("count")),
      name,
      timeLimitMinutes: durationHours * 60,
      authorizedGuestLimit: map(data.get("guests"), parseNumber),
      code: map(data.get("code"), parseText),
      remarks: map(data.get("remarks"), parseText),
    };

    try {
      const res = await api.createVoucher(payload);
      setNewVouchers(res.vouchers);
      notify(`Successfully created ${res.vouchers.length} vouchers`, "success");
      form.reset();
      setCount(MIN_VOUCHER_COUNT);
    } catch (error: any) {
      if (error?.status === 409) {
        notify("That key is already in use", "error");
      } else if (error?.status === 400) {
        notify(
          "The controller does not accept that voucher's name, key or remarks",
          "error",
        );
      } else {
        notify("Failed to create voucher", "error");
      }
    }
    setLoading(false);
  };

  const closeModal = useCallback(() => {
    setNewVouchers(null);
  }, []);

  const isBatch = count > 1;

  return (
    <div>
      <form onSubmit={handleSubmit} className="card max-w-lg mx-auto space-y-6">
        <div>
          <label className="block font-medium mb-1">Number</label>
          <input
            name="count"
            type="number"
            required
            min={MIN_VOUCHER_COUNT}
            max={MAX_VOUCHER_COUNT}
            value={count}
            onChange={(e) => setCount(Number(e.target.value) || 0)}
          />
        </div>

        <div>
          <label className="block font-medium mb-1">Name</label>
          <input
            name="name"
            type="text"
            required={!isBatch}
            disabled={isBatch}
            defaultValue="Custom Voucher"
          />
          {isBatch && (
            <p className="text-sm text-secondary mt-1">
              The controller names batch vouchers itself (Guest-1, Guest-2,
              ...).
            </p>
          )}
        </div>

        <div>
          <label className="block font-medium mb-1">Duration</label>
          <div className="flex-center gap-2">
            <input
              name="duration"
              type="number"
              required
              min={1}
              step={1}
              max={Math.floor(
                MAX_VOUCHER_DURATION_HOURS / HOURS_PER_UNIT[durationUnit],
              )}
              defaultValue={24}
            />
            <select
              name="durationUnit"
              onChange={(e) => setDurationUnit(e.target.value as TimeUnit)}
              className="w-auto"
              defaultValue="hours"
            >
              <option value="hours">Hours</option>
              <option value="days">Days</option>
              <option value="weeks">Weeks</option>
            </select>
          </div>
        </div>

        <div>
          <label className="block font-medium mb-1">Guest Limit</label>
          <input
            name="guests"
            type="number"
            min={MIN_VOUCHER_GUESTS}
            max={MAX_VOUCHER_GUESTS}
            placeholder="Unlimited"
          />
        </div>

        <div>
          <label className="block font-medium mb-1">Key</label>
          <input
            name="code"
            type="text"
            disabled={isBatch}
            minLength={MIN_VOUCHER_KEY_LENGTH}
            maxLength={MAX_VOUCHER_KEY_LENGTH}
            pattern={"[^\\s#&+\"'<>,]+"}
            title={`${MIN_VOUCHER_KEY_LENGTH} to ${MAX_VOUCHER_KEY_LENGTH} characters, no spaces or # & + " ' < > ,`}
            placeholder={isBatch ? "Generated per voucher" : "Generated"}
          />
        </div>

        <div>
          <label className="block font-medium mb-1">Remarks</label>
          <input name="remarks" type="text" placeholder="None" />
        </div>

        <button type="submit" disabled={loading} className="btn-primary w-full">
          {loading ? "Creating…" : "Create Custom Voucher"}
        </button>
      </form>
      {newVouchers && (
        <SuccessModal vouchers={newVouchers} onClose={closeModal} />
      )}
    </div>
  );
}
