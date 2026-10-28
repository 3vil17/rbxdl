<p align="center">
  <img src="src/web/logo.png" width="96" alt="rbxdl logo" />
</p>

# rbxdl

Roblox asset downloader that runs locally. Start the exe, it opens in your browser,
paste an asset id or link, and it saves the file. Everything goes straight from your
machine to Roblox, there's no server in between.

Works with audio, images, decals, meshes, models, animations and pretty much anything
else that has an asset id. Decals get followed to the actual image.

## Installation

Download `rbxdl.exe` from [releases](https://github.com/3vil17/rbxdl/releases) and run it.
Windows only for now.

Or build it (Rust + MSVC build tools):

```
cargo build --release
```

The exe ends up in `target/release/rbxdl.exe`.

## Usage

When you run it, a browser tab opens. If it doesn't, the console window prints the
address to open.

- **Download** - paste an id or any roblox.com / create.roblox.com link, hit Fetch to
  preview it, then Download.
- **Bulk** - paste up to 500 ids or links (spaces, commas or new lines all work).
  Shows progress per asset and you can cancel halfway.
- **Accounts** - every cookie you added, with its avatar, whether it still works and
  how many requests it has done.
- **Settings** - API key, cookies and the download folder.
- **History** - past downloads with a button to open the file in Explorer.

Files go to `Downloads/RBXDL` by default, you can pick another folder in Settings.

## Credentials

Most assets need you to be logged in, so add at least one of these in Settings.
They're stored in Windows Credential Manager, not in a file.

- **Open Cloud API key** with the `legacy-asset:manage` scope. Make one at
  create.roblox.com/dashboard/credentials. Safest option since it can't do anything
  else with your account.
- **`.ROBLOSECURITY` cookies**, one per line. These get you more assets than the API
  key. Adding a few (alts are fine) makes bulk downloads faster, requests get spread
  across them and if one gets rate limited the others keep going.

Don't share your cookie with anyone, it's a full login to your account.

## When a download fails

It tells you what Roblox said for each thing it tried, for example `403 User is not
authorized to access Asset`. Usually it means the asset is private and none of your
accounts have access, or the asset got moderated or deleted.

## Notes

Personal project, still early. Issues and PRs welcome.
