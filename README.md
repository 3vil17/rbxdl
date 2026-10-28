<p align="center">
  <img src="src/web/logo.png" width="96" alt="rbxdl logo" />
</p>

# rbxdl

Roblox asset downloader that runs locally. Start the exe, it opens in your browser,
paste an asset id or link, and it saves the file.

## Installation

Download `rbxdl.exe` from [releases](https://github.com/3vil17/rbxdl/releases) and run it.

Or build it (Rust + MSVC build tools):

```
cargo build --release
```

## Usage

- **Download** - paste an id or link, Fetch, Download.
- **Bulk** - up to 500 ids at once.
- **Accounts** - which cookies are being used and their status.
- **Settings** - API key, cookies (one per line) and the download folder.
- **History** - past downloads.

Files go to `Downloads/RBXDL` by default.

## Credentials

You need at least one of these. They're stored in Windows Credential Manager.

- Open Cloud API key with the `legacy-asset:manage` scope.
- `.ROBLOSECURITY` cookies. Add a few if you're bulk downloading, requests get spread
  across them so you don't get rate limited as fast.

If a download fails it shows the actual error from Roblox for each thing it tried.
