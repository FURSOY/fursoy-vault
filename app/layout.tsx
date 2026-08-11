import type { Metadata } from "next";
import { Manrope, Space_Mono } from "next/font/google";
import "./globals.css";

const manrope = Manrope({ variable: "--font-manrope", subsets: ["latin"] });
const spaceMono = Space_Mono({ variable: "--font-mono", subsets: ["latin"], weight: ["400", "700"] });

export const metadata: Metadata = {
  metadataBase: new URL("https://fursoy.com"),
  title: { default: "FURSOY Vault — Local session protection", template: "%s · FURSOY Vault" },
  description: "Protect selected Chrome sessions in a local encrypted Windows vault and restore them only after Windows Hello approval.",
  keywords: ["FURSOY Vault", "Windows Hello", "Chrome session protection", "local cookie vault", "open source security"],
  openGraph: { title: "FURSOY Vault", description: "Close the session. Keep the access.", url: "https://fursoy.com", siteName: "FURSOY Vault", images: [{ url: "/og.png", width: 1731, height: 909 }], type: "website" },
  twitter: { card: "summary_large_image", title: "FURSOY Vault", description: "Close the session. Keep the access.", images: ["/og.png"] },
  icons: { icon: "/app-icon.png", apple: "/app-icon.png" },
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <html lang="en"><body className={`${manrope.variable} ${spaceMono.variable}`}>{children}</body></html>;
}
