// Writes latest.json, the file the in-app updater reads, from the signed installers in a folder.
// Usage: node scripts/make-update-manifest.mjs <folder> <version>
// An installer counts only when its signature (.sig) is beside it. With none, no file is written
// and the app falls back to opening the release page.
import { existsSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [folder, version] = process.argv.slice(2);
if (!folder || !version) {
  console.error("Usage: node scripts/make-update-manifest.mjs <folder> <version>");
  process.exit(1);
}

// Which installer updates which platform. The macOS build is universal, so it serves both.
const TARGETS = [
  { ends: "_x64-setup.exe", platforms: ["windows-x86_64"] },
  { ends: ".AppImage", platforms: ["linux-x86_64"] },
  { ends: ".app.tar.gz", platforms: ["darwin-aarch64", "darwin-x86_64"] },
];

const base = `https://github.com/KRSLYS/AI-Balance/releases/download/v${version}`;
const platforms = {};
for (const name of readdirSync(folder)) {
  const target = TARGETS.find((candidate) => name.endsWith(candidate.ends));
  const signaturePath = join(folder, `${name}.sig`);
  if (!target || !existsSync(signaturePath)) continue;
  const signature = readFileSync(signaturePath, "utf8").trim();
  for (const platform of target.platforms) {
    platforms[platform] = { signature, url: `${base}/${encodeURIComponent(name)}` };
  }
}

// The signatures now live in latest.json; the loose files would only clutter the release.
for (const name of readdirSync(folder)) {
  if (name.endsWith(".sig")) rmSync(join(folder, name));
}

if (Object.keys(platforms).length === 0) {
  console.log("No signed installers: no latest.json written.");
  process.exit(0);
}
const manifest = { version, pub_date: new Date().toISOString(), platforms };
writeFileSync(join(folder, "latest.json"), JSON.stringify(manifest, null, 2) + "\n");
console.log(`latest.json covers ${Object.keys(platforms).join(", ")}`);
