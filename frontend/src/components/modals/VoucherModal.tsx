"use client";

import Modal from "@/components/modals/Modal";
import VoucherCode from "@/components/utils/VoucherCode";
import { GuestPass } from "@/types/voucher";
import {
  formatDateTime,
  formatDevices,
  formatDurationSecs,
  formatStatus,
} from "@/utils/format";
import { ReactNode } from "react";

type Props = {
  pass: GuestPass;
  onClose: () => void;
};

export default function VoucherModal({ pass, onClose }: Props) {
  const rows: [string, ReactNode][] = [
    ["Status", formatStatus(pass.used)],
    ["Name", pass.name || "No note"],
    ["SSID", pass.ssid],
    ["Created", formatDateTime(pass.createdAt)],
    ...(pass.activatedAt != null
      ? ([["Activated", formatDateTime(pass.activatedAt)]] as [
          string,
          ReactNode,
        ][])
      : []),
    [
      pass.used ? "Expires" : "Must be claimed by",
      formatDateTime(pass.expiresAt),
    ],
    ["Session Length", formatDurationSecs(pass.validTimeSecs)],
    ["Devices", formatDevices(pass.shareNumber)],
    ["Connected Devices", String(pass.clientMacs.length)],
    ["ID", pass.id],
  ];

  return (
    <Modal onClose={onClose}>
      <VoucherCode pass={pass} contentClassName="mb-8" />
      <div className="space-y-4">
        {rows.map(([label, value]) => (
          <div
            key={label}
            className="flex-center-between p-4 bg-interactive border border-subtle rounded-xl space-x-4"
          >
            <span className="font-semibold text-primary">{label}:</span>
            <span className="text-secondary">{value}</span>
          </div>
        ))}
      </div>
    </Modal>
  );
}
