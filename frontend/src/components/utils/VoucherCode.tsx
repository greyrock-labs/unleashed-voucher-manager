import { copyText } from "@/utils/clipboard";
import { formatCode } from "@/utils/format";
import { notify } from "@/utils/notifications";
import { useState } from "react";
import { GuestPass } from "@/types/voucher";

type Props = {
  pass: GuestPass;
  contentClassName?: string;
};

export default function VoucherCode({ pass, contentClassName = "" }: Props) {
  const code = formatCode(pass.code);
  const [_copied, setCopied] = useState(false);

  const handleCopy = async () => {
    if (await copyText(pass.code)) {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
      notify("Code copied to clipboard!", "success");
    } else {
      notify("Failed to copy code", "error");
    }
  };

  return (
    <div className={`text-center ${contentClassName}`}>
      <div
        onClick={handleCopy}
        className="cursor-pointer mb-4 text-3xl voucher-code"
      >
        {code}
      </div>
      <div className="flex-center gap-3">
        <button onClick={handleCopy} className="btn-success">
          Copy Code
        </button>
      </div>
    </div>
  );
}
