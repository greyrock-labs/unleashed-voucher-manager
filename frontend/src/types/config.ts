export type RuntimeConfig = {
  IS_LOGO_INVERTIBLE: boolean;
  WIFI_SSID?: string;
  WIFI_PASSWORD?: string;
  WIFI_TYPE?: string;
  WIFI_HIDDEN?: string;
};

export const DEFAULT_RUNTIME_CONFIG: RuntimeConfig = {
  IS_LOGO_INVERTIBLE: false,
};
