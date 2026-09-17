import { NextResponse, NextRequest } from "next/server";

export const config = {
  matcher: ["/rust-api/:path*"],
};

const DEFAULT_FRONTEND_TO_BACKEND_URL = "http://127.0.0.1";
const DEFAULT_BACKEND_BIND_PORT = "8080";

export function proxy(request: NextRequest) {
  const backend =
    process.env.FRONTEND_TO_BACKEND_URL ?? DEFAULT_FRONTEND_TO_BACKEND_URL;
  const port = process.env.BACKEND_BIND_PORT ?? DEFAULT_BACKEND_BIND_PORT;

  const url = new URL(request.nextUrl.pathname.replace(/^\/rust-api/, "/api"), `${backend}:${port}`);
  url.search = request.nextUrl.search;

  return NextResponse.rewrite(url);
}
