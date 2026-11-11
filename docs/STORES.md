# Publishing to Google Play and the App Store

What is ready in the code, what you still have to do yourself, and what a reviewer may object to.

## Ready in the code

- **Privacy policy:** [PRIVACY.md](../PRIVACY.md), linked from Settings. Both stores require a public
  link to it; use its GitHub address.
- **No data reaches the developer:** no analytics, no crash reporting, no server.
- **Sample data:** the empty dashboard offers "Try with sample data", so a reviewer can see the app
  working without owning any AI subscription. Say so in the review notes.
- **Permissions:** network access, and notifications only after the user turns them on in Settings.
- **Signed Android app bundle:** with the `ANDROID_KEYSTORE_*` secrets set, the release workflow
  attaches a signed `.aab` for Google Play next to the `.apk`.
- **Signed iOS build:** with the `APPLE_*` secrets set, the release workflow builds an `.ipa` for
  App Store Connect.
- **Languages:** 12, with right-to-left support.

## You have to do

### Both stores

1. Accounts: Google Play Console (a one-time $25) and the Apple Developer Program ($99 a year).
2. Store listing: name, short and long description, screenshots for phone and tablet sizes, a
   1024×1024 icon (`assets/icon.png`), a support address and the privacy policy link.
3. Check that "AI Balance" is free to use as a name in each store and is not someone's trademark.
4. Have a native speaker read each translation you list the app in.

### Google Play

1. Create an upload keystore once (the command is in the README) and add the three
   `ANDROID_KEYSTORE_*` secrets. Keep the keystore safe: every update needs it.
2. Fill in the **Data safety** form. Without ads: no data collected or shared. With ads: declare
   what the ad SDK collects (device or other IDs, approximate location, app interactions,
   diagnostics), shared with the ad network, for advertising.
3. Fill in the content rating and target audience (not for children), and declare whether the app
   contains ads.
4. A new personal developer account must run a closed test with at least 12 testers for 14 days
   before it can publish to production.

### App Store

1. Add the four `APPLE_*` secrets, create the app in App Store Connect with the bundle identifier
   `com.karimseyam.aibalance`, and upload the `.ipa` with Transporter.
2. Fill in **App Privacy**. Without ads: "Data not collected". With ads: identifiers, usage data and
   coarse location, used for third-party advertising, and "used to track you" if ads are
   personalized.
3. Answer the export compliance question: the app uses only standard HTTPS encryption, which is
   exempt.
4. In the review notes, explain that the app reads the user's own usage from their own accounts,
   and point to the sample data.

## Ads are not in the code yet

Showing ads needs things only you can provide, and a native SDK that has to be tested on real
phones:

1. An AdMob account, an AdMob app ID for Android and one for iOS, and a banner ad unit ID for each.
2. The Google Mobile Ads SDK added to the generated Android and iOS projects, with Google's consent
   form (required in the EEA, the UK and Switzerland) and, on iOS, the tracking permission prompt
   and privacy manifest.
3. An `app-ads.txt` file on a website you own, listed as the developer website in both stores.

The plan that keeps ads out of the way: one small banner at the bottom of the Usage tab, on Android
and iOS only, never over content, never full-screen, and none on desktop.

## What a reviewer may object to

Read this before paying for the developer accounts.

- **Other companies' sign-ins.** Claude, ChatGPT, Cursor, Copilot and Kimi publish no sign-in for
  third-party apps and no public usage API. AI Balance signs in the way those companies' own
  command-line tools do and reads the same usage endpoints. Apple (guideline 5.2.2) and Google
  (impersonation and intellectual property policies) can reject an app that uses a third-party
  service without that service's permission, and a provider can ask a store to remove it.
- **The providers' own terms.** Some of these providers restrict using their subscription sign-in
  outside their own apps. A provider could block the app's requests at any time, and in the worst
  case act against an account that uses it.
- **Lower-risk option.** OpenRouter documents its key and credits API, and Z.ai and Ollama use API
  keys the user creates for this purpose. A store version limited to API-key providers is much
  easier to defend in review; the full set can stay in the desktop and direct-download versions.
- **Provider names.** The app shows provider names as plain text and uses no logos. Keep store
  screenshots and the description free of logos, and do not imply the providers endorse the app.
- **Ads beside account data.** Keep the banner clearly separate from the usage cards so an ad can
  never be mistaken for app content.
