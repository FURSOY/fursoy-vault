import type { MetadataRoute } from "next";

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: "FURSOY Vault",
    short_name: "FURSOY Vault",
    description: "Local session protection for Chrome on Windows.",
    start_url: "/",
    display: "browser",
    background_color: "#F6F8F5",
    theme_color: "#0B3334",
    categories: ["security", "utilities", "productivity"],
    icons: [{ src: "/app-icon.png", sizes: "1214x1214", type: "image/png" }],
  };
}
