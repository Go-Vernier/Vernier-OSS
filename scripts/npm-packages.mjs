// Writes the npm packages for one release into <out-dir>:
//   cli/               @go-vernier/cli, the launcher, with the version stamped in
//   cli-<os>-<cpu>/    one per target, holding that target's binary
//
//   node scripts/npm-packages.mjs <version> <bins-dir> <out-dir>
//
// <bins-dir>/bin-<triple>/vernier[.exe] is what the release build uploads.

import { chmodSync, copyFileSync, cpSync, existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { TARGETS, binaryName, npmDir, npmName } from "./targets.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const MAIN = path.join(ROOT, "npm", "cli");
const LICENSE = path.join(ROOT, "LICENSE");
const REPOSITORY = { type: "git", url: "git+https://github.com/Go-Vernier/Vernier-OSS.git" };

export function normalizeVersion(version) {
  const v = String(version).replace(/^v/, "");
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z.-]+)?$/.test(v)) {
    throw new Error(`"${version}" is not a release version`);
  }
  return v;
}

const writeJson = (file, value) => writeFileSync(file, `${JSON.stringify(value, null, 2)}\n`);

export function writePackages(version, binsDir, outDir) {
  const v = normalizeVersion(version);
  const binaries = TARGETS.map((t) => {
    const file = path.join(binsDir, `bin-${t.triple}`, binaryName(t));
    if (!existsSync(file)) throw new Error(`no binary for ${t.triple} at ${file}`);
    return { t, file };
  });
  if (existsSync(outDir) && readdirSync(outDir).length > 0) {
    throw new Error(`${outDir} must be empty`);
  }
  mkdirSync(outDir, { recursive: true });

  const main = path.join(outDir, "cli");
  cpSync(MAIN, main, {
    recursive: true,
    filter: (src) => !["test", "node_modules"].includes(path.relative(MAIN, src).split(path.sep)[0]),
  });
  copyFileSync(LICENSE, path.join(main, "LICENSE"));
  const manifest = JSON.parse(readFileSync(path.join(main, "package.json"), "utf8"));
  manifest.version = v;
  manifest.optionalDependencies = Object.fromEntries(TARGETS.map((t) => [npmName(t), v]));
  writeJson(path.join(main, "package.json"), manifest);

  const dirs = [main];
  for (const { t, file } of binaries) {
    const dir = path.join(outDir, npmDir(t));
    mkdirSync(path.join(dir, "bin"), { recursive: true });
    const bin = path.join(dir, "bin", binaryName(t));
    copyFileSync(file, bin);
    chmodSync(bin, 0o755);
    copyFileSync(LICENSE, path.join(dir, "LICENSE"));
    writeFileSync(
      path.join(dir, "README.md"),
      `# ${npmName(t)}\n\nThe \`vernier\` binary for ${t.os} ${t.cpu}. Install ` +
        "[`@go-vernier/cli`](https://www.npmjs.com/package/@go-vernier/cli) instead; " +
        "it picks the right binary for you.\n",
    );
    writeJson(path.join(dir, "package.json"), {
      name: npmName(t),
      version: v,
      description: `The vernier binary for ${t.os} ${t.cpu}. Install @go-vernier/cli instead.`,
      license: "MIT",
      repository: REPOSITORY,
      os: [t.os],
      cpu: [t.cpu],
      files: ["bin"],
      preferUnplugged: true,
    });
    dirs.push(dir);
  }
  return dirs;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [version, binsDir, outDir] = process.argv.slice(2);
  if (!outDir) {
    console.error("usage: node scripts/npm-packages.mjs <version> <bins-dir> <out-dir>");
    process.exit(2);
  }
  try {
    for (const dir of writePackages(version, binsDir, outDir)) console.log(dir);
  } catch (error) {
    console.error(`npm-packages: ${error.message}`);
    process.exit(1);
  }
}
