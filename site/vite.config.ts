import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { basename } from "node:path";
import sharp from "sharp";
import { defineConfig, type Plugin } from "vite";
import { SITE } from "./src/content/data.ts";

// `image.png?trim` crops a logo from the repository's assets to its visible pixels.
function trim(): Plugin {
  let build = false;
  return {
    name: "trim",
    enforce: "pre",
    configResolved(config) {
      build = config.command === "build";
    },
    async load(id) {
      const [file, query] = id.split("?");
      if (query !== "trim" || file.endsWith(".svg")) return;
      this.addWatchFile(file);
      const source = await sharp(file).trim().png().toBuffer();
      if (!build)
        return `export default ${JSON.stringify(`data:image/png;base64,${source.toString("base64")}`)}`;
      const ref = this.emitFile({
        type: "asset",
        name: basename(file),
        source,
      });
      return `export default import.meta.ROLLUP_FILE_URL_${ref}`;
    },
  };
}

export default defineConfig({
  base: new URL(SITE).pathname,
  plugins: [trim(), react(), tailwindcss()],
  server: { fs: { allow: [".."] } },
});
