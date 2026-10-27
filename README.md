# rbxdl

A little Roblox asset downloader that runs on your own machine. You start the exe,
it opens a page in your browser at a localhost address, you paste an asset id or a
roblox.com link, and it saves the file to disk. No third party server sits in the
middle, the assets come straight from Roblox.

The release binary is about 1.5 MB and has no runtime dependencies.

## Running it

Grab `rbxdl.exe` (or build it, see below) and run it. A browser tab opens on its own.
If it doesn't, the console prints the address, something like `http://127.0.0.1:52341`.

Paste an id or any Roblox asset URL, hit Fetch to see what it is, then Download.

Files land in your Downloads/RBXDL folder by default. You can change that under
Settings, and every download shows up under History with a button to open its folder.

## Credentials

Roblox won't hand over most assets anonymously, so you give it one of two things in
Settings. Both are kept in Windows Credential Manager, not in a file on disk.

- An Open Cloud API key. Make one at create.roblox.com with the `legacy-asset:manage`
  scope. This is the safe option, it can't touch your account, Robux or inventory.
- Your `.ROBLOSECURITY` cookie. Needed for restricted assets and the place id trick
  below. There's a "Check cookie" button that tells you whose it is and if it's alive.

## How it finds an asset

For each asset it tries these in order and stops at the first that works:

1. Open Cloud, using the API key.
2. A direct cookie request to assetdelivery.roblox.com.
3. The place id fallback. It looks up who made the asset, lists the games that
   creator owns, and retries the request with each game's `Roblox-Place-Id`. Roblox
   will serve an asset if you ask for it in the context of a place that actually uses
   it. The result line tells you which method (and which place) let it through.

Step 3 only works with a cookie, the Open Cloud route has no place context.

Decals and images sometimes come back as a little XML descriptor pointing at the real
image id. When that happens it follows the pointer once and grabs the actual image.

## Build

Needs the Rust toolchain with the MSVC build tools on Windows.

```
cargo run              # debug
cargo build --release  # optimized exe at target/release/rbxdl.exe
```

## Notes

Personal project, pre 1.0. It does what I need but there are rough edges, so don't lean
on it for anything important yet.
