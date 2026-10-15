// Bundles the client into <outdir>/app.js and copies index.html beside it.
// lifecore-server's build.rs runs this with OUT_DIR; `npm run build` and
// `npm run watch` write dist/ for `lifecore serve --ui-dir dist`.
import { copyFile, mkdir } from "node:fs/promises";
import path from "node:path";
import { parseArgs } from "node:util";
import * as esbuild from "esbuild";

const { values } = parseArgs({
  options: {
    outdir: { type: "string", default: "dist" },
    watch: { type: "boolean", default: false },
  },
});
const outdir = path.resolve(values.outdir);
await mkdir(outdir, { recursive: true });

const copyIndex = {
  name: "copy-index",
  setup(build) {
    build.onEnd(async () => {
      await copyFile("index.html", path.join(outdir, "index.html"));
    });
  },
};

const options = {
  entryPoints: ["src/main.tsx"],
  bundle: true,
  outfile: path.join(outdir, "app.js"),
  format: "iife",
  target: "es2022",
  jsx: "automatic",
  minify: !values.watch,
  define: {
    "process.env.NODE_ENV": JSON.stringify(
      values.watch ? "development" : "production",
    ),
  },
  logLevel: "warning",
  plugins: [copyIndex],
};

if (values.watch) {
  const context = await esbuild.context(options);
  await context.watch();
} else {
  await esbuild.build(options);
}
