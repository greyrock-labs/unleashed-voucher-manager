import { NextResponse, NextRequest } from "next/server";
import { isInBlockedSubnet } from "@/utils/ipv4";

export const config = {
  matcher: ["/", "/rust-api/:path*"],
};

const DEFAULT_FRONTEND_TO_BACKEND_URL = "http://127.0.0.1";
const DEFAULT_BACKEND_BIND_PORT = "8080";

const IPV6_IPV4_MAPPED_PREFIX = "::ffff:";

const guestAllowedPaths = [
  "/welcome",
  "/rust-api/vouchers/rolling",
  "favicon.ico",
  "favicon.svg",
];

export function proxy(request: NextRequest) {
  const { pathname } = request.nextUrl;

  // Extract client IP: the last X-Forwarded-For entry, which the reverse
  // proxy in front of the app appended. Earlier entries come from the client
  // and can be forged.
  let clientIp = (request.headers.get("x-forwarded-for") || "")
    .split(",")
    .pop()!
    .trim();

  // Strip IPv6 prefix if it's a mapped IPv4
  if (clientIp.startsWith(IPV6_IPV4_MAPPED_PREFIX)) {
    clientIp = clientIp.replace(IPV6_IPV4_MAPPED_PREFIX, "");
  }

  // Restrict access based on GUEST_SUBNETWORK env variable
  const guestSubnet = process.env.GUEST_SUBNETWORK;
  if (guestSubnet) {
    if (
      !guestAllowedPaths.includes(pathname) &&
      isInBlockedSubnet(clientIp, guestSubnet)
    ) {
      return new NextResponse("Access denied", { status: 403 });
    }
  }

  if (pathname.startsWith("/rust-api")) {
    // Remove the /rust-api prefix and reconstruct the path for the backend
    const backendPath = request.nextUrl.pathname.replace(/^\/rust-api/, "/api");

    const backendUrl =
      process.env.FRONTEND_TO_BACKEND_URL || DEFAULT_FRONTEND_TO_BACKEND_URL;
    const backendPort =
      process.env.BACKEND_BIND_PORT || DEFAULT_BACKEND_BIND_PORT;

    const backendFullUrl = new URL(
      `${backendUrl}:${backendPort}${backendPath}${request.nextUrl.search}`,
    );

    // Forward only the client IP chosen above, so the backend never sees the
    // forgeable entries
    const headers = new Headers(request.headers);
    headers.set("x-forwarded-for", clientIp);
    return NextResponse.rewrite(backendFullUrl, { request: { headers } });
  }

  return NextResponse.next();
}
