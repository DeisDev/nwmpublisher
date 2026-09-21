<p align="center">
  <img src="public/img/logo.svg" alt="nwmpublisher logo" width="128" height="128">
</p>

# ⚙️ nwmpublisher (no watermark publisher)

A fork of [gmpublisher](https://github.com/WilliamVenner/gmpublisher).
At it's core, it's still the same feature-packed workshop publisher for Garry's Mod, but now with some improvements made in various areas such as quality of life and stability.

The main difference is that there is no more gmpublisher advertising/branding such as the "Uploaded with gmpublisher" links in your description when you first upload an addon. You get to control the name of the .gma file (no longer gmpublisher.gma), the icon, description and changelog all from the first publish.

Aside from this, there is also an assortment of bug fixes, technical changes and overall improvements to the user experience that might make you want to switch over to nwmpublisher.

Maintained by [DeisDev](https://github.com/DeisDev) ("CatSniffer").

Built on the work of [William Venner](https://github.com/WilliamVenner) and the
[gmpublisher contributors](https://github.com/WilliamVenner/gmpublisher/graphs/contributors).

## 📦 Installation

Download the latest release from the [releases page](https://github.com/DeisDev/nwmpublisher/releases).

| Platform | Download | Install |
| --- | --- | --- |
| Windows | `.msi` | Open the installer. |
| macOS, Intel or Apple Silicon | Universal `.dmg` | Open the disk image, drag nwmpublisher into Applications, then launch it from Applications. |
| Ubuntu / Debian | `amd64.deb` | Open a terminal in your download folder and run `sudo apt install ./nwmpublisher_*.deb`. This also installs required system libraries. |
| Fedora | `x86_64.rpm` | Run `sudo dnf install ./nwmpublisher-*.rpm` in your download folder. |
| Other Linux distributions / Steam Deck | `amd64.AppImage` | In the file's properties, allow it to run as a program, then open it. On Steam Deck, use Desktop Mode. |








### Upgrading from gmpublisher

On Windows, installing nwmpublisher **replaces** an existing gmpublisher installation. The installer
uses the same upgrade code as gmpublisher, so Windows uninstalls gmpublisher and its shortcuts
first — the two can't be installed side by side.

Your settings are left where they are. On first launch nwmpublisher finds them and offers to import
them — destinations, local addon paths and preferences — then restarts with them applied.

To move them yourself instead, copy `settings.json` from the old `gmpublisher` folder into the
`nwmpublisher` folder in your config directory (`%APPDATA%` on Windows, `~/.config` on Linux,
`~/Library/Application Support` on macOS).


## Features

* Doesn't depend on gmad.exe or gmpublish.exe
* Publish & update your Workshop items without added branding
* Choose your GMA filenames and use your Steam avatar as the default icon
* Edit descriptions & changelogs with BBCode, live previews, fullscreen editing and undo/redo
* Update descriptions without reuploading addon files
* Filter & sort your Workshop items, with your choices saved between sessions
* Drag addon folders into the publishing file browser
* Extract, search and browse GMA files and installed addons
* View file counts and sizes for entire folders
* Bulk download & extract Workshop items and collections
* Upload animated GIFs as your Workshop item's icon
* Analyze which addons are taking up the most disk space using the addon size analyzer treemap
* Supports legacy SteamPipe addons and old GMA versions
* Works without an Internet connection
* Choose whether to open Workshop pages after publishing and folders after extraction
* Copy diagnostics from failed jobs
* CLI extraction with custom output paths and an option to keep the folder closed
* (Windows) .GMA file type association for quick extraction

## Command line

Run `nwmpublisher` without arguments to launch the GUI. The CLI supports extraction,
help, and version information:

```sh
nwmpublisher --help
nwmpublisher --version
nwmpublisher --extract "addon.gma"
nwmpublisher --extract "addon.gma" --out "extracted-addon" --no-open
```

`-e` and `-o` are aliases for `--extract` and `--out`. Output goes directly into
`--out`, creating the directory if needed and overwriting matching files. Without
`--out`, extraction uses an addon-named folder in the application's configured
temporary directory (by default, `nwmpublisher` inside the system temporary directory).
This default destination follows the application's overwrite/recycle setting.

Successful extraction prints the output path. The folder opens according to the
application's **Open folder after extraction** preference; `--no-open` always suppresses
opening it for that invocation. Use `--no-open` for scripts or environments without a desktop.

Exit codes are `0` for success (including help and version), `1` for archive,
extraction, or folder-opening failures, and `2` for invalid arguments. Failures print
details to standard error. A folder-opening failure leaves the extracted files available.

In Windows PowerShell scripts, wait for the GUI-subsystem executable and read its exit code:

```powershell
$cliProcess = Start-Process -FilePath .\nwmpublisher.exe -ArgumentList '--extract "addon.gma" --out "extracted-addon" --no-open' -NoNewWindow -Wait -PassThru
$cliProcess.ExitCode
```

Publishing and updating Workshop items are currently available only in the GUI.

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

## Supported Platforms

* Windows
* macOS
* Linux

Windows is the primary development and testing platform.

## Media

![Screenshot](public\screenshots\MyWorkshop.png)

![Screenshot](public\screenshots\Publish.png)

![Screenshot](public\screenshots\Update.png)

![Screenshot](public\screenshots\DescriptionEditor.png)

![Screenshot](public\screenshots\Installed.png)

![Screenshot](public\screenshots\Extract.png)

![Screenshot](public\screenshots\Size.png)

![Screenshot](public\screenshots\Settings.png)

<p align="center"><img src="https://i.imgur.com/Un4akZe.gif"/></p>
