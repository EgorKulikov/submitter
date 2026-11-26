# Submitter Helper (browser extension)

Optional companion to the `submitter` CLI. Automates paste + language
select + click on the AtCoder, Codeforces, and Luogu submit pages.

## Install (unpacked, recommended for now)

### Chrome / Edge / Brave

1. Download `submitter-extension-<version>.zip` from the latest
   [submitter release](https://github.com/EgorKulikov/submitter/releases).
2. Unzip it to a folder you'll keep around (the extension loads from
   that folder).
3. Open `chrome://extensions`, enable **Developer mode**, click **Load
   unpacked**, select the unzipped folder.

### Firefox

1. Download and unzip as above.
2. Open `about:debugging#/runtime/this-firefox`.
3. Click **Load Temporary Add-on**, select the unzipped folder's
   `manifest.json`.

Firefox temporary add-ons disappear on browser restart, and release
Firefox only installs add-ons signed by Mozilla. To keep the extension,
sign your own copy as an unlisted add-on (free, automatic, never listed
on the store):

1. In the unzipped folder's `manifest.json`, replace
   `browser_specific_settings.gecko.id` with an ID of your own, such as
   `{<a random UUID>}`. Signing ties the ID to the account that signs it.
2. Zip the folder's contents, with `manifest.json` at the root of the zip.
3. On [addons.mozilla.org](https://addons.mozilla.org/developers/),
   choose **Submit a New Add-on**, then **On your own**, and upload the zip.
4. Once it is signed, download the `.xpi` and install it from
   `about:addons` → gear icon → **Install Add-on From File**.
5. In `about:addons` → Submitter Helper → **Permissions**, check that
   every site is allowed, `127.0.0.1` included.

To update it later, raise `version` in `manifest.json` and upload it as a
new version of the same add-on.

## How it works

When `submitter` opens the submit page, it appends a one-shot URL
fragment like `#submitter=PORT:TOKEN`. The extension reads it, fetches
the source from a short-lived loopback server in `submitter`, then
pastes + picks the language + clicks Submit. If anything goes wrong, a
yellow banner explains and the source is still on your clipboard.

The extension is purely additive — uninstall it and `submitter` works
exactly as before.

## Privacy

- No network requests except to `http://127.0.0.1:<port>` (your own
  machine) and the three site domains the extension is permitted on.
- No tracking, no analytics, no remote config.

## Languages

The map of language names lives in `shared/languages.js`. If your
language isn't matched, the extension pastes the code and leaves
language picking to you. PRs to extend the map are welcome.
