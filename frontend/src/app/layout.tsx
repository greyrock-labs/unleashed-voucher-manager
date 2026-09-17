import { GlobalProvider } from "@/contexts/GlobalContext";
import "./globals.css";
import type { Metadata } from "next";

export const metadata: Metadata = {
  title: "Unleashed Voucher Manager",
  description: "Manage Ruckus Unleashed guest passes with ease",
  authors: [{ name: "Greyrock Labs", url: "https://github.com/greyrock-labs" }],
  creator: "Greyrock Labs",
  robots: {
    index: false,
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en" data-scroll-behavior="smooth" suppressHydrationWarning>
      <body className="antialiased">
        <GlobalProvider>{children}</GlobalProvider>
      </body>
    </html>
  );
}
