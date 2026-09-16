// Validate subnet format and values
export function isValidSubnet(subnet: string): boolean {
  try {
    // Handle single IP addresses (treat as /32)
    if (!subnet.includes("/")) {
      return isValidIPAddress(subnet);
    }

    // Parse the subnet (e.g., "10.0.5.0/24")
    const [subnetIp, prefixLength] = subnet.split("/");

    if (!subnetIp || !prefixLength) {
      return false;
    }

    // Validate prefix length
    const prefix = parseInt(prefixLength, 10);
    if (isNaN(prefix) || prefix < 0 || prefix > 32) {
      return false;
    }

    // Validate IP address format
    if (!isValidIPAddress(subnetIp)) {
      return false;
    }

    // Check if the IP is a valid network address
    const ipToInt = (ipAddr: string): number => {
      const parts = ipAddr.split(".");
      return (
        parts.reduce((acc, part) => {
          const num = parseInt(part, 10);
          return (acc << 8) + num;
        }, 0) >>> 0
      );
    };

    const subnetIpInt = ipToInt(subnetIp);
    const mask = (0xffffffff << (32 - prefix)) >>> 0;
    const networkAddress = (subnetIpInt & mask) >>> 0;

    // Check if the provided IP is actually the network address
    // Comment out these lines if you want to allow any IP in subnet notation
    if (subnetIpInt !== networkAddress) {
      console.warn(
        `IP ${subnetIp} is not a network address for /${prefix}. Expected: ${intToIp(networkAddress)}`,
      );
      return false;
    }

    return true;
  } catch (error) {
    console.error(
      `Error checking subnet: ${error instanceof Error ? error.message : "Unknown error"}`,
    );
    return false;
  }
}

// Utility export function to validate IP address format
// Taken from https://stackoverflow.com/a/36760050
export function isValidIPAddress(ip: string): boolean {
  const ipRegex = /^((25[0-5]|(2[0-4]|1\d|[1-9]|)\d)\.?\b){4}$/;
  return ipRegex.test(ip);
}

// Helper export function to convert integer back to IP string
export function intToIp(int: number): string {
  return [
    (int >>> 24) & 255,
    (int >>> 16) & 255,
    (int >>> 8) & 255,
    int & 255,
  ].join(".");
}

// Robust subnet check with proper CIDR notation support
export function isInBlockedSubnet(ip: string, subnet: string): boolean {
  if (!ip || !subnet) return false;

  // Validate inputs first
  if (!isValidIPAddress(ip)) {
    console.warn(`Invalid IP address format: ${ip}`);
    return false;
  }

  if (!isValidSubnet(subnet)) {
    console.warn(`Invalid subnet format: ${subnet}`);
    return false;
  }

  try {
    // Normalize subnet (add /32 for single IPs)
    const normalizedSubnet = subnet.includes("/") ? subnet : `${subnet}/32`;
    const [subnetIp, prefixLength] = normalizedSubnet.split("/");
    const prefix = parseInt(prefixLength, 10);

    // Convert IP addresses to 32-bit integers
    const ipToInt = (ipAddr: string): number => {
      const parts = ipAddr.split(".");
      return (
        parts.reduce((acc, part) => {
          const num = parseInt(part, 10);
          return (acc << 8) + num;
        }, 0) >>> 0
      );
    };

    const targetIpInt = ipToInt(ip);
    const subnetIpInt = ipToInt(subnetIp);

    // Create subnet mask
    const mask = (0xffffffff << (32 - prefix)) >>> 0;

    // Check if the IP is in the subnet
    return (targetIpInt & mask) === (subnetIpInt & mask);
  } catch (error) {
    console.error(
      `Error checking subnet: ${error instanceof Error ? error.message : "Unknown error"}`,
    );
    return false;
  }
}

// Enhanced version with support for multiple subnets
export function isInAnyBlockedSubnet(ip: string, subnets: string[]): boolean {
  if (subnets.length === 0) return false;

  // Validate IP once
  if (!isValidIPAddress(ip)) {
    console.warn(`Invalid IP address format: ${ip}`);
    return false;
  }

  // Filter valid subnets and check
  const validSubnets = subnets.filter((subnet) => {
    const isValid = isValidSubnet(subnet);
    if (!isValid) {
      console.warn(`Skipping invalid subnet: ${subnet}`);
    }
    return isValid;
  });

  return validSubnets.some((subnet) => isInBlockedSubnet(ip, subnet));
}
