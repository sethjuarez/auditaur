// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";

export default defineConfig({
  site: "https://auditaur.dev",
  markdown: {
    gfm: true,
  },
  integrations: [
    starlight({
      title: "Auditaur",
      customCss: ["./src/styles/custom.css"],
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/sethjuarez/auditaur",
        },
      ],
      sidebar: [
        { label: "Welcome", link: "/welcome/" },
        { label: "Getting Started", items: [{ autogenerate: { directory: "getting-started" } }] },
        { label: "Concepts", items: [{ autogenerate: { directory: "concepts" } }] },
        { label: "Reference", items: [{ autogenerate: { directory: "reference" } }] },
        { label: "Roadmap", items: [{ autogenerate: { directory: "roadmap" } }] },
      ],
    }),
  ],
});
