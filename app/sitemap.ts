import type { MetadataRoute } from "next";

const lastModified = new Date("2026-08-11T00:00:00.000Z");

export default function sitemap(): MetadataRoute.Sitemap {
  return [
    { url: "https://fursoy.com", lastModified, changeFrequency: "weekly", priority: 1 },
    { url: "https://fursoy.com/privacy", lastModified, changeFrequency: "monthly", priority: 0.6 },
  ];
}
