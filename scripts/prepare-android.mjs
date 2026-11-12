// Prepares the Android project that `tauri android init` generates: adds the home-screen widget
// and the credential vault, and turns cloud backup off.
// Run it after every init: the generated project is not committed.
import { cpSync, existsSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";

const source = "src-tauri/android";
const main = "src-tauri/gen/android/app/src/main";

/** The generated MainActivity.kt, wherever the package puts it. */
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
if (!activityPath) {
  console.error("No generated Android project. Run `npx tauri android init` first.");
  process.exit(1);
}

let activity = readFileSync(activityPath, "utf8");
const packageName = activity.match(/^package\s+([\w.]+)/m)?.[1];
if (!packageName) {
  console.error("Could not read the package name from MainActivity.kt");
  process.exit(1);
}

// The widget class, in the app's own package.
const widget = readFileSync(join(source, "UsageWidget.kt"), "utf8").replace("__PACKAGE__", packageName);
writeFileSync(join(dirname(activityPath), "UsageWidget.kt"), widget);

// The plugin that encrypts the saved sign-ins with a key from the Android Keystore.
const vault = readFileSync(join(source, "VaultPlugin.kt"), "utf8").replace("__PACKAGE__", packageName);
writeFileSync(join(dirname(activityPath), "VaultPlugin.kt"), vault);

// Its layout, drawables, colors and strings.
cpSync(join(source, "res"), join(main, "res"), { recursive: true });

// Redraw the widget whenever the app goes to the background, with what it just refreshed.
if (!activity.includes("UsageWidget.refresh")) {
  const end = activity.lastIndexOf("}");
  activity = `${activity.slice(0, end)}
  override fun onPause() {
    super.onPause()
    UsageWidget.refresh(this)
  }
}
`;
  writeFileSync(activityPath, activity);
}

// Keep the app's data, the saved sign-ins included, out of Google's cloud backup.
{
  let manifest = readFileSync(join(main, "AndroidManifest.xml"), "utf8");
  if (/android:allowBackup="[^"]*"/.test(manifest)) {
    manifest = manifest.replace(/android:allowBackup="[^"]*"/, 'android:allowBackup="false"');
  } else {
    manifest = manifest.replace("<application", '<application android:allowBackup="false"');
  }
  writeFileSync(join(main, "AndroidManifest.xml"), manifest);
}

// Declare the widget.
const manifestPath = join(main, "AndroidManifest.xml");
let manifest = readFileSync(manifestPath, "utf8");
if (!manifest.includes(".UsageWidget")) {
  const receiver = `        <receiver android:name=".UsageWidget" android:exported="false" android:label="AI Balance">
            <intent-filter>
                <action android:name="android.appwidget.action.APPWIDGET_UPDATE" />
            </intent-filter>
            <meta-data android:name="android.appwidget.provider" android:resource="@xml/usage_widget_info" />
        </receiver>
    </application>`;
  if (!manifest.includes("</application>")) {
    console.error("AndroidManifest.xml has no </application> tag");
    process.exit(1);
  }
  manifest = manifest.replace("</application>", receiver.trimStart());
  writeFileSync(manifestPath, manifest);
}

console.log(`Added the widget to ${packageName}`);
