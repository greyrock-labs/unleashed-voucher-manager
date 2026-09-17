export interface GuestPass {
  id: string;
  name: string;
  code: string;
  ssid: string;
  createdAt: number;
  /** null until the guest first uses the pass */
  activatedAt: number | null;
  /**
   * When network access ends -- createdAt + validTimeSecs, claimed or not,
   * given the creation-time validity the README requires of the controller.
   */
  expiresAt: number;
  /** seconds of access the pass grants, counted from creation */
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
