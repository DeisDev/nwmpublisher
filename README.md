<p align="center">
  <img src="public/img/logo.svg" alt="nwmpublisher logo" width="128" height="128">
</p>

# ⚙️ nwmpublisher (no watermark publisher)

A fork of [gmpublisher](https://github.com/WilliamVenner/gmpublisher) with the branding stripped out.
Underneath it's the same feature-packed Workshop publisher for Garry's Mod.

Maintained by [DeisDev](https://github.com/DeisDev) ("CatSniffer").

## What's different

* **The name.** The executable, installer, window title, CLI, config and temp folders, registry
  entries and `.gma` file association all say nwmpublisher.
* **Your GMA files are named by you.** The packed file follows the addon title you type in the
  publish window, can be renamed to whatever you like, and falls back to `publishedaddon.gma` when
  left empty. No more `gmpublisher.gma`.
* **The default Workshop preview icon is your own Steam avatar**, kept square without borders or
  shadows, instead of the gmpublisher logo.
* **Nothing you publish mentions gmpublisher** — not the item description, and not your Steam status,
  which reads "In the Workshop".

Everything else is upstream gmpublisher, so the documentation below still applies. All credit for the
app itself goes to [William Venner](https://github.com/WilliamVenner) and the
[gmpublisher contributors](https://github.com/WilliamVenner/gmpublisher/graphs/contributors).

## 📦 Installation

Download the latest release from the [releases page](https://github.com/DeisDev/nwmpublisher/releases).

### Upgrading from gmpublisher

On Windows, installing nwmpublisher **replaces** an existing gmpublisher installation. The installer
uses the same upgrade code as gmpublisher, so Windows uninstalls gmpublisher and its shortcuts
first — the two can't be installed side by side.

Your settings are left where they are. On first launch nwmpublisher finds them and offers to import
them — destinations, local addon paths and preferences — then restarts with them applied.

To move them yourself instead, copy `settings.json` from the old `gmpublisher` folder into the
`nwmpublisher` folder in your config directory (`%APPDATA%` on Windows, `~/.config` on Linux,
`~/Library/Application Support` on macOS).

## Tutorials

[DanFMN - Fastest Way to Upload a Garry's Mod Addon To Workshop](https://youtu.be/_syLXTFXmgM)

[DarkFated - GMPublisher Guide in Russian](https://youtu.be/ldjHm85AnYk)

## Features

* Doesn't depend on gmad.exe or gmpublish.exe
* Publish & update your Workshop items
* Extract, search and browse GMA files and installed addons
* Bulk download & extract Workshop items and collections
* Upload animated GIFs as your Workshop item's icon
* Analyze which addons are taking up the most disk space using the addon size analyzer treemap
* Supports legacy SteamPipe addons and old GMA versions
* Works without an Internet connection
* CLI interface
* (Windows) .GMA file type association for quick extraction

## Languages

![](https://user-images.githubusercontent.com/14863743/115954244-ce459780-a4e7-11eb-9237-92eab7d17814.png) English

![](https://user-images.githubusercontent.com/14863743/115954306-195faa80-a4e8-11eb-8489-07ceca216211.png) French

![](https://user-images.githubusercontent.com/14863743/115954290-03ea8080-a4e8-11eb-86df-9001929981a7.png) German

![](https://user-images.githubusercontent.com/14863743/115957563-18844400-a4fb-11eb-9828-cf76b15c6a48.png) Russian

![](https://user-images.githubusercontent.com/14863743/116080210-ad6c7600-a690-11eb-8c26-33de913e7ad0.png) Polish

![](https://user-images.githubusercontent.com/14863743/115975014-223c9480-a559-11eb-81c4-6a0bfc0fdb9d.png) Turkish

![](https://user-images.githubusercontent.com/14863743/116463612-cfb5ed80-a862-11eb-81f1-fb453cf77da5.png) Portuguese (Brazil)

![](https://user-images.githubusercontent.com/14863743/115976530-d7297e00-a566-11eb-9fe0-113c59ce49ce.png) Spanish

![](https://user-images.githubusercontent.com/14863743/123729167-754e0300-d88c-11eb-9dae-6fb82e0ca0ce.png) Chinese

![](https://user-images.githubusercontent.com/14863743/123729280-9dd5fd00-d88c-11eb-8aee-0360615d4d57.png) Dutch

![](https://github.com/WilliamVenner/gmpublisher/assets/14863743/31a1a199-1427-483c-bf6c-140116e3f445) Korean

![](https://github.com/Blueberryy/gmpublisher/assets/36592509/319e7681-46c4-4a79-9fdc-99db49bd2ccb) Ukrainian


[Want to translate nwmpublisher to your language?](i18n)

## Requirements

Windows, macOS or Linux

Linux users may need to install additional dependencies.

## Technical Stuff

* The program makes heavy use of multithreading, and will work best on processors with a decent amount of cores.
* Made using [Rust](https://www.rust-lang.org/) (backend) and [Svelte](https://svelte.dev/) (frontend)
* This is not an Electron app; this is a [Tauri](https://github.com/tauri-apps/tauri) app. Big thanks to all the contributors to Tauri for their amazing work on finally killing Electron for good.
* nwmpublisher uses the fantastic [steamworks-rs](https://crates.io/crates/steamworks) library for interfacing with the [Steamworks SDK](https://partner.steamgames.com/doc/api)
* The program is only about ~10 MB

## Media

![Screenshot](https://user-images.githubusercontent.com/14863743/115953601-5f1a7400-a4e4-11eb-831c-d6a924afbf33.png)

![Screenshot](https://user-images.githubusercontent.com/14863743/115953605-63469180-a4e4-11eb-9f96-90b992cbffc4.png)

![Screenshot](https://user-images.githubusercontent.com/14863743/115954341-5b88ec00-a4e8-11eb-8f27-c03d43df165a.png)

![Screenshot](https://user-images.githubusercontent.com/14863743/115953616-7c4f4280-a4e4-11eb-95c0-add80b1d41bd.png)

![Screenshot](https://user-images.githubusercontent.com/14863743/115953639-9db02e80-a4e4-11eb-935d-bad41cd30bde.png)

![Screenshot](https://user-images.githubusercontent.com/14863743/115958825-00afbe80-a501-11eb-81da-6d53a94eddbf.png)

![Screenshot](https://user-images.githubusercontent.com/14863743/115953801-845bb200-a4e5-11eb-8fc2-8b142f2be237.png)

![Screenshot](https://user-images.githubusercontent.com/14863743/115953820-99d0dc00-a4e5-11eb-93a4-36e8b2248e87.png)

![Screenshot](https://user-images.githubusercontent.com/14863743/115953827-a35a4400-a4e5-11eb-9691-48e520eb9bb1.png)

![Screenshot](https://user-images.githubusercontent.com/14863743/115953670-bb7d9380-a4e4-11eb-8f54-f43fcd153d90.png)

<p align="center"><img src="https://i.imgur.com/Un4akZe.gif"/></p>
