// Adds the ad code to the generated Android project, for the store edition only.
// Usage: node scripts/add-android-ads.mjs live|test
// "test" uses Google's public test ad identifiers, which are safe to tap; "live" uses the real ones.
// The full edition is built before this runs, so it contains no ad code at all.
import { existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const mode = process.argv[2];
if (mode !== "live" && mode !== "test") {
  console.error("Usage: node scripts/add-android-ads.mjs live|test");
  process.exit(1);
}
const ids = JSON.parse(readFileSync("src-tauri/ads.json", "utf8"))[mode === "live" ? "android" : "test"];
const app = "src-tauri/gen/android/app";
const main = join(app, "src/main");

function findActivity(dir) {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      const found = findActivity(path);
      if (found) return found;
    } else if (name === "MainActivity.kt") {
      return path;
    }
  }
  return null;
}

const activityPath = existsSync(join(main, "java")) ? findActivity(join(main, "java")) : null;
const packageName = activityPath && readFileSync(activityPath, "utf8").match(/^package\s+([\w.]+)/m)?.[1];
if (!packageName) {
  console.error("No generated Android project. Run `npx tauri android init` first.");
  process.exit(1);
}

// The plugin, in the app's own package.
const plugin = readFileSync("src-tauri/android-ads/AdsPlugin.kt", "utf8").replace("__PACKAGE__", packageName);
writeFileSync(join(dirname(activityPath), "AdsPlugin.kt"), plugin);

// Google's ad and consent libraries.
const gradlePath = join(app, "build.gradle.kts");
let gradle = readFileSync(gradlePath, "utf8");
if (!gradle.includes("play-services-ads")) {
  if (!gradle.includes("dependencies {")) {
    console.error("build.gradle.kts has no dependencies block");
    process.exit(1);
  }
  gradle = gradle.replace(
    "dependencies {",
    `dependencies {
    implementation("com.google.android.gms:play-services-ads:23.6.0")
    implementation("com.google.android.ump:user-messaging-platform:3.1.0")`,
  );
  writeFileSync(gradlePath, gradle);
}

// The AdMob app this build belongs to.
const manifestPath = join(main, "AndroidManifest.xml");
let manifest = readFileSync(manifestPath, "utf8");
const tag = `<meta-data android:name="com.google.android.gms.ads.APPLICATION_ID" android:value="${ids.appId}" />`;
const existing = /<meta-data android:name="com\.google\.android\.gms\.ads\.APPLICATION_ID"[^>]*\/>/;
if (existing.test(manifest)) {
  manifest = manifest.replace(existing, tag);
} else if (manifest.includes("</application>")) {
  manifest = manifest.replace("</application>", `    ${tag}\n    </application>`);
} else {
  console.error("AndroidManifest.xml has no </application> tag");
  process.exit(1);
}
writeFileSync(manifestPath, manifest);

console.log(`Added ${mode} ads to ${packageName}`);
