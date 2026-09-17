import Modal from "@/components/modals/Modal";
import VoucherCode from "@/components/utils/VoucherCode";
import { GuestPass } from "@/types/voucher";

type Props = {
  pass: GuestPass;
  onClose: () => void;
};

export default function SuccessModal({ pass, onClose }: Props) {
  return (
    <Modal onClose={onClose} contentClassName="max-w-sm">
      <h2 className="text-2xl font-bold text-primary mb-4 text-center">
        Guest Pass Created!
      </h2>
      <VoucherCode pass={pass} />
    </Modal>
  );
}
