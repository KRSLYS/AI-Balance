// Usage: npm run set-version -- 1.2.3
// Writes the version to package.json, src-tauri/Cargo.toml and src-tauri/tauri.conf.json.
import { readFileSync, writeFileSync } from "node:fs";

const version = (process.argv[2] || "").replace(/^v/, "");
if (!/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/.test(version)) {
  console.error(`Invalid version "${process.argv[2] ?? ""}". Expected something like 1.2.3`);
  process.exit(1);
}

const json = (path, edit) => {
  const data = JSON.parse(readFileSync(path, "utf8"));
  edit(data);
  writeFileSync(path, JSON.stringify(data, null, 2) + "\n");
};

json("package.json", (p) => (p.version = version));
json("src-tauri/tauri.conf.json", (c) => (c.version = version));

const cargoPath = "src-tauri/Cargo.toml";
const cargo = readFileSync(cargoPath, "utf8");
const next = cargo.replace(/^version = ".*"$/m, `version = "${version}"`);
if (next === cargo && !cargo.includes(`version = "${version}"`)) {
  console.error("Could not find the version line in Cargo.toml");
  process.exit(1);
}
writeFileSync(cargoPath, next);
console.log(`Version set to ${version}`);
