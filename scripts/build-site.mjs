// Builds the website into _site/: the pages in site/, plus privacy.html made from PRIVACY.md,
// so the policy has one source.
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { marked } from "marked";

rmSync("_site", { recursive: true, force: true });
mkdirSync("_site");
cpSync("site", "_site", { recursive: true });

const policy = marked.parse(readFileSync("PRIVACY.md", "utf8"));
writeFileSync(
  "_site/privacy.html",
  `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Privacy Policy · AI Balance</title>
    <link rel="icon" href="assets/icon.svg" type="image/svg+xml" />
    <link rel="stylesheet" href="styles.css" />
  </head>
  <body>
    <div class="wrap">
      <header class="nav">
        <a class="brand" href="./"><img src="assets/icon.svg" width="34" height="34" alt="" />AI Balance</a>
      </header>
    </div>
    <main class="doc">
${policy}
    </main>
  </body>
</html>
`,
);
console.log("Built _site");
