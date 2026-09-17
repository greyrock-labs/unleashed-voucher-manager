import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",

  experimental: {
    // TypeScript 7 dropped the compiler API Next.js drives in-process, so
    // next build refuses to typecheck with it and says to set this. It
    // shells out to tsc instead, which 7 still ships. Remove once Next
    // supports 7 natively.
    useTypeScriptCli: true,
  },
};

export default nextConfig;
