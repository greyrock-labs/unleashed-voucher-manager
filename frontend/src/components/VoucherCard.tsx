import { GuestPass } from "@/types/voucher";
import { formatCode, formatDateTime, formatDevices } from "@/utils/format";
import { memo, useCallback } from "react";

type Props = {
  pass: GuestPass;
  onClick?: (p: GuestPass) => void;
};

const VoucherCard = ({ pass, onClick }: Props) => {
  const statusClass = pass.used
    ? "bg-status-warning text-status-warning"
    : "bg-status-success text-status-success";
  const onClickHandler = useCallback(() => onClick?.(pass), [pass, onClick]);

  return (
    <div onClick={onClickHandler} className="card card-interactive">
      {/* Primary Information */}
      <div className="mb-2">
        <div className="text-xl voucher-code">{formatCode(pass.code)}</div>
        <div className="text-lg font-semibold truncate">{pass.name}</div>
      </div>

      <div className="space-y-1 text-sm text-secondary">
        <div className="flex justify-between">
          <span>SSID:</span>
          <span>{pass.ssid}</span>
        </div>

        <div className="flex justify-between">
          <span>Devices:</span>
          <span>{formatDevices(pass.shareNumber)}</span>
        </div>

        <div className="flex justify-between">
          <span>Created:</span>
          <span className="text-xs">{formatDateTime(pass.createdAt)}</span>
        </div>

        <div className="flex justify-between">
          <span>Connected Devices:</span>
          <span>{pass.clientMacs.length}</span>
        </div>

        <div className="flex-center-between">
          <span
            className={`px-2 py-1 rounded-lg text-xs font-semibold uppercase ${statusClass}`}
          >
            {pass.used ? "Used" : "Available"}
          </span>
          {/* Always "Expires": with the controller on creation-time validity
              (see README, Controller setup) an unclaimed pass has a real
              expiry too, so the old "Must be claimed by" label was wrong. */}
          <span className="text-xs">
            Expires: {formatDateTime(pass.expiresAt)}
          </span>
        </div>
      </div>
    </div>
  );
};

export default memo(VoucherCard);
