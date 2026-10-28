<p align="center">
  <img src="src/web/logo.png" width="96" alt="rbxdl logo" />
</p>

# rbxdl

A little Roblox asset downloader that runs on your own machine. You start the exe,
it opens a page in your browser at a localhost address, you paste an asset id or a
roblox.com link, and it saves the file to disk. No third party server sits in the
middle, the assets come straight from Roblox.

## Installation

Grab `rbxdl.exe` from the [releases page](https://github.com/3vil17/rbxdl/releases)
and run it. That's it, no installer and no runtime dependencies, the exe is about 1.5 MB.

Or build it yourself. Needs the Rust toolchain with the MSVC build tools on Windows.

```
cargo run              # debug
cargo build --release  # optimized exe at target/release/rbxdl.exe
```

## Running it

Run the exe and a browser tab opens on its own. If it doesn't, the console prints
the address, something like `http://127.0.0.1:52341`.

Paste an id or any Roblox asset URL, hit Fetch to see what it is, then Download.
For lots at once, the Bulk tab takes up to 500 ids or links in one go.

Files land in your Downloads/RBXDL folder by default. You can change that under
Settings, and every download shows up under History with a button to open its folder.

## Credentials

Roblox won't hand over most assets anonymously, so you give it one of two things in
Settings. Both are kept in Windows Credential Manager, not in a file on disk.

- An Open Cloud API key. Make one at create.roblox.com with the `legacy-asset:manage`
  scope. This is the safe option, it can't touch your account, Robux or inventory.
- One or more `.ROBLOSECURITY` cookies, one per line. Needed for restricted assets and
  the place id trick below. The Accounts tab shows whose each cookie is, whether it's
  alive, and how much it's being used.

## Rate limits

Roblox doesn't publish a limit for assetdelivery, so rbxdl plays it safe. Cookie
requests rotate across your live accounts, each account sends at most one request
every 350 ms, and when Roblox answers 429 that account cools down for the
`Retry-After` time (or 5s, doubling up to 2 minutes) while the others keep going.
More cookies means faster bulk downloads.

## How it finds an asset

For each asset it tries these in order and stops at the first that works:

1. Open Cloud, using the API key.
2. A direct cookie request to assetdelivery.roblox.com, with each account.
3. The place id fallback. It looks up who made the asset, lists the games that
   creator owns, and retries the request with each game's `Roblox-Place-Id`. Roblox
   will serve an asset if you ask for it in the context of a place that actually uses
   it. The result line tells you which method, account and place let it through.

Step 3 only works with a cookie, the Open Cloud route has no place context.

If nothing works you get what each step got back from Roblox (status and message),
not just a generic "not accessible".

Decals and images sometimes come back as a little XML descriptor pointing at the real
image id. When that happens it follows the pointer once and grabs the actual image.

## Notes

Personal project, pre 1.0. It does what I need but there are rough edges, so don't lean
on it for anything important yet.
