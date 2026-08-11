import type { MetadataRoute } from "next";
export default function sitemap(): MetadataRoute.Sitemap { return [{ url: "https://fursoy.com", lastModified: new Date(), priority: 1 }, { url: "https://fursoy.com/privacy", lastModified: new Date(), priority: .7 }]; }
