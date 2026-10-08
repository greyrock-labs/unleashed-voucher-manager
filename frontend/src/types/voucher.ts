export interface Voucher {
  id: string;
  createdAt: string;
  name: string;
  code: string;
  authorizedGuestLimit?: number | null;
  authorizedGuestCount: number;
  activatedAt?: string | null;
  expiresAt?: string | null;
  expired: boolean;
  timeLimitMinutes: number;
  remarks: string;
}

export interface VoucherCreateData {
  count: number;
  name: string;
  timeLimitMinutes: number;
  authorizedGuestLimit?: number | null;
  /** A custom pass key; only allowed when count is 1. */
  code?: string | null;
  remarks?: string | null;
}

export interface VoucherGetResponse {
  offset: number;
  limit: number;
  count: number;
  totalCount: number;
  data: Voucher[];
}

export interface VoucherDeletedResponse {
  vouchersDeleted: number;
}

export interface VoucherCreatedResponse {
  vouchers: Voucher[];
}
