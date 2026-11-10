// @ts-check
import { defineConfig } from "astro/config";
import starlight from "@astrojs/starlight";
import { remarkHeadingId } from "remark-custom-heading-id";

// https://astro.build/config
export default defineConfig({
  markdown: {
    remarkPlugins: [remarkHeadingId],
  },
  integrations: [
    starlight({
      title: "komorebi",
      logo: {
        src: "./src/assets/watermelon.svg",
        alt: "komorebi",
      },
      favicon: "/favicon.svg",
      social: [
        {
          icon: "github",
          label: "GitHub",
          href: "https://github.com/LGUG2Z/komorebi",
        },
      ],
      sidebar: [
        {
          label: "About",
          items: [
            { label: "Overview", slug: "about/overview" },
            { label: "Community", slug: "about/community" },
            { label: "Licensing", slug: "about/licensing" },
            { label: "Sponsorship", slug: "about/sponsorship" },
            { label: "Expectations", slug: "about/expectations" },
          ],
        },
        {
          label: "Guides",
          items: [
            { label: "Installation", slug: "guides/installation" },
            {
              label: "License Registration",
              slug: "guides/license-registration",
            },
            { label: "Quickstart", slug: "guides/quickstart" },
            { label: "Upgrades", slug: "guides/upgrades" },
            { label: "Known Issues", slug: "guides/known-issues" },
            { label: "Uninstallation", slug: "guides/uninstallation" },
          ],
        },
        {
          label: "Usage",
          items: [
            { label: "Windows", slug: "usage/windows" },
            { label: "Containers", slug: "usage/containers" },
            { label: "Workspaces", slug: "usage/workspaces" },
            { label: "Monitors", slug: "usage/monitors" },
            { label: "Layouts", slug: "usage/layouts" },
            { label: "Padding", slug: "usage/padding" },
            { label: "Layers", slug: "usage/layers" },
            { label: "Borders", slug: "usage/borders" }, // borders and colors
            { label: "Themes", slug: "usage/themes" }, // themes for borders (and the bar)
            { label: "Animations", slug: "usage/animations" }, // experimental animations
            { label: "Wallpapers", slug: "usage/wallpapers" }, // wallpapers per monitor/workspace and themegen
            { label: "Rules", slug: "usage/rules" }, // layout rules, behavior rules, matching rules, composite rules
          ],
        },
        {
          label: "Reference",
          autogenerate: { directory: "reference" },
        },
        {
          label: "Common Workflows",
          autogenerate: { directory: "common-workflows" },
        },
      ],
    }),
  ],
});
