# Contributing

Thanks for helping with nwmpublisher. Bug reports, suggestions, and questions go in [Issues](https://github.com/DeisDev/nwmpublisher/issues/new/choose). Problems with the original gmpublisher belong in [its repository](https://github.com/WilliamVenner/gmpublisher/issues).

## Setup

You need:

- Node.js 26.9.0 (see `.node-version`) and npm
- Rust through [rustup](https://rustup.rs), which uses the toolchain pinned in `src-tauri/rust-toolchain.toml`
- The [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your platform

Then install dependencies from the repository root:

```sh
npm ci
```

## Run and test

| Task | Directory | Command |
| --- | --- | --- |
| Run the desktop app | root | `npm run tauri -- dev` |
| Build the frontend | root | `npm run build` |
| Run the website | root | `npm run dev:site` |
| Build the website | root | `npm run build:site` |
| Run Rust tests | `src-tauri/` | `cargo test --locked` |

Steam must be running to test publishing and downloads. `npm run dev` alone starts only the web frontend, which does not work without Tauri.

## Pull requests

- Keep each pull request focused on one change.
- Follow `.editorconfig`: tabs, UTF-8, and a final newline.
- Put desktop app text in `i18n/en.json` and website text in `site/i18n/en.json`.
- Add a short line under `Unreleased` in `CHANGELOG.md` for changes users will notice.
- Use [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/) for commit messages, such as `fix(publish): Keep the icon after a failed upload`.
- Make sure the builds and Rust tests above pass, and say how you tested the change.
