import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  async rewrites() {
    return [
      {
        source: '/api/:path*',
        destination: 'http://localhost:8080/api/:path*',
      },
      {
        source: '/docs/:path*',
        destination: 'http://localhost:8080/docs/:path*',
      },
    ];
  },
};

export default nextConfig;
