# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- Fix macOS release verification failing after disk image creation because the architecture check passed arguments in the wrong order.

## [3.1.0] - 2026-09-21

### Added

- Add `--no-open` to CLI extraction to suppress opening the output folder for that invocation.
- Filter My Workshop by Public, Friends only, Private, or Unlisted visibility, and sort the full collection by current subscribers, last updated, publication date, or title in either direction. Remember both choices across sessions.
- Show recursive file counts and sizes beside folders in the file browser, with footer totals for the current directory.
- Choose separately whether to open Workshop pages after publishing and folders after single-addon extraction. Existing behavior is preserved by default; required Steam agreement prompts and explicit Open actions remain available.
- Copy per-job diagnostics from failed publishing, downloading, and extraction jobs.
- Update an existing addon's description without selecting or uploading addon files, and open its previous change notes on Steam from the Changelog tab.
- Undo and redo typing, pasting, and BBCode formatting in both editors, using buttons or Ctrl/Cmd+Z, Ctrl+Y, and Ctrl/Cmd+Shift+Z. Each editor keeps its history across tab and fullscreen changes.
- Edit Workshop descriptions as plain text or BBCode when publishing or updating addons, with buttons for common Steam formatting. Untouched descriptions remain unchanged; invalid or oversized descriptions are rejected before upload.
- Include an optional changelog on an addon's initial Workshop upload, with Steam BBCode formatting buttons available for both initial uploads and updates.
- Preview description and changelog formatting as you type, with an Image BBCode button and images scaled to fit without cropping or stretching.
- Expand description and changelog editors to fill the app window with their live previews. Return with Back or Escape, and use Ctrl/Cmd shortcuts for every formatting button (shown in tooltips).
- Add buttons and shortcuts for horizontal rules, attributed quotes, literal BBCode, and tables in both editors, with table previews supporting borderless and equal-width columns.

### Changed

- Format file sizes and transfer rates using the selected application language and consistent IEC binary units (KiB, MiB, and KiB/s) throughout the interface.
- Replace the Linux ZIP with AppImage, DEB and RPM packages, and the macOS ZIP with a universal drag-to-Applications DMG. Bundle Steam's library with working launch paths, support optional Mac signing and notarization, and publish all platforms together with checksums after package checks pass.
- Upgrade to Tauri 2, Svelte 5, Vite 8, Rust 1.98.1, Node.js 26.9.0, and npm 12.0.2. Linux builds now use WebKitGTK 4.1.
- Replace the interface icons with Lucide icons.
- Use one main upload button with a dropdown for description-only updates or packaging and updating an addon. Remember the selected mode, and show exactly what the button will send.
- Keep the default Workshop preview image square, without circular cropping, borders, or shadows.
- Give the publishing file browser its own full-height tab, alongside Description and Changelog. Formatting toolbars and ignored-file patterns start collapsed.
- Use the new logo throughout the app, favicon, and packaged application icons.

### Fixed

- Document only supported CLI commands, report archive and extraction failures with their paths, and return nonzero exit codes for failed operations or invalid arguments. Show command-line output in the parent Windows console.
- Enable dropping one addon folder into the publishing File Browser using the same validation as browsing. Reject multiple paths and ignore drops when the browser is hidden or publishing is in progress.
- Clear Steam launch variables after connecting on Windows so browsers and folder windows opened by nwmpublisher do not inherit Garry's Mod's Steam identity and keep it marked as running after closing the publisher.
- Use the same normalized file manifest for publishing previews and packing, reject duplicate archive paths, and revalidate content and exclusions when publishing starts. Report traversal and whitelist errors instead of silently omitting files.
- Cancel publishing during preparation and packing, acknowledge cancellation before unlocking the publisher, and disable cancellation once Steam submission starts. Keep active uploads visible and release the publishing lock when a publishing command is rejected.
- Isolate each publishing job's archive and generated icons in its own temporary directory, upload only its GMA, and clean up after success, failure, or cancellation. Sanitize archive names consistently across platforms, including reserved names and uppercase extensions.
- Fix inconsistent addon ordering in the size analyzer and Rust compiler compatibility warnings.
- Preserve the operation, path, and underlying error when publishing or extracting files fails. Report extraction completion only after files and metadata are written and flushed, reject truncated archive reads, and return icon-generation errors instead of crashing or silently using another image.
- Show Steam error details in downloader tooltips for both separate error data and combined `KEY:details` messages.
- Keep the publishing details and upload button visible at the default window size by adapting the image preview height and tightening spacing.
- Replaced the old logo in the Windows installer's header banner.

## [3.0.0] - 2026-09-17

### Added

- Added an optional desktop shortcut to the Windows installer's feature selection.
- On first launch, nwmpublisher detects the settings left behind by an older gmpublisher install
  and offers to import them (destinations, local addon paths and preferences). Declining writes out
  fresh settings so the offer isn't repeated.

### Changed

- Reworded the support popup to credit Billy for GLua Enhanced and use a neutral dismissal label.
- **Renamed the app to `nwmpublisher`.** This covers the executable and installer, the window title,
  the CLI name, the `.gma` file association and MSI registry entries, the config, temp and log
  folders, and the WebSocket subprotocol.
- The update checker and the GitHub links now point at
  [DeisDev/nwmpublisher](https://github.com/DeisDev/nwmpublisher) instead of upstream gmpublisher.
- The Windows installer shares gmpublisher's upgrade code, so installing it replaces an existing
  gmpublisher installation. Settings are left in place and offered for import on first launch.
- Started an independent version line at 3.0.0.
- The installer matches every version of the shared product family and removes what it finds, so it
  can always be installed over an existing gmpublisher build, newer versions included.
- Set DeisDev as the MSI publisher and retained William Venner in the author credits.
- Published GMA files are no longer always named `gmpublisher.gma`. The file name now follows the
  addon title entered in the publish window, and can be overridden with any name you like.
- If no name is given, the packed file is named `publishedaddon.gma`.
- The default Workshop preview icon is now your Steam avatar with a white outline around it, instead
  of the gmpublisher logo.

### Removed

- Newly published Workshop items no longer get "Uploaded with gmpublisher" written into their
  description.
- The Steam status shown while the app is running no longer reads "In gmpublisher"; it now reads
  "In the Workshop".

### Fixed

- Fixed the support popup's links to open in the default browser.
- Fixed remaining Rust build warnings and updated Steam discovery and WebSocket dependencies to remove future-compatibility warnings.
- Fixed MSI linking for the `.gma` icon, validation of optional features, and installer registry paths.
- Load current settings before mounting the interface after an import, and show migration
  read or save failures without treating the migration as completed.
- The gmpublisher settings prompt no longer comes back after importing. It used to reappear on the
  restart that follows the import and only went away once the app was closed and opened again.
- Fixed MSI metadata, optional file associations, and shortcut registration.
- Embedded the WebView2 bootstrapper for checked, silent runtime installation.
- Fixed failed update checks and notification cleanup.
- Fixed MSI icon linking and unused-result warnings in release builds.
- Updated panic hooks and removed unnecessary mutable-static and unused bindings.
