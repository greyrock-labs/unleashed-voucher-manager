export interface GuestPass {
  id: string;
  name: string;
  code: string;
  ssid: string;
  createdAt: number;
  /** null until the guest first uses the pass */
  activatedAt: number | null;
  /**
   * For an unused pass this is the deadline to first use (fixed by the
   * controller at 7 days). For a used pass it is activatedAt + validTimeSecs.
   */
  expiresAt: number;
  /** seconds of access granted once the pass is first used */
  validTimeSecs: number;
  used: boolean;
  /** 0 means unlimited devices */
  shareNumber: number;
  clientMacs: string[];
  remarks: string;
}

export interface PassCreateData {
  name: string;
  durationHours: number;
  shareNumber: number;
}
